import argparse
import json
import os
import pathlib
import random
import re
import subprocess
import sys

HERE = pathlib.Path(__file__).resolve().parent

RUBRIC = """You grade two answers to a university programming languages (UCLA CS131) homework question. You do not know which system wrote which.
Score each answer from 0 to 10 on:
- correct: every claim, value and line of code is right (code would compile and run as described)
- complete: it covers every point in the checklist and every part the question asks for, without the student needing to ask a follow-up
- clear: a student can follow it and learn the method
Reply with only JSON: {"A": {"correct": n, "complete": n, "clear": n, "missing": "short note"}, "B": {...}}"""


def grade(question, first, second, model):
    prompt = (
        f"{RUBRIC}\n\n<question>\n{question['prompt']}\n</question>\n\n"
        f"<checklist>\n" + "\n".join(f"- {m}" for m in question["must"]) + "\n</checklist>\n\n"
        f"<answer_A>\n{first}\n</answer_A>\n\n<answer_B>\n{second}\n</answer_B>"
    )
    done = subprocess.run(
        ["claude", "-p", "--model", model, "--output-format", "text"],
        input=prompt, capture_output=True, text=True, timeout=900,
    )
    found = re.search(r"\{.*\}", done.stdout, re.S)
    if not found:
        raise RuntimeError(done.stderr.strip() or done.stdout.strip())
    return json.loads(found.group(0))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("left", help="label:kind, e.g. base-medium:raw")
    parser.add_argument("right", help="label:kind, e.g. base-medium:felix")
    parser.add_argument("--model", default="claude-opus-5-5")
    parser.add_argument("--out", default=os.environ.get("FELIX_EVAL_OUT", str(HERE / "out")))
    args = parser.parse_args()
    questions = json.loads((HERE / "questions.json").read_text())
    sides = [args.left, args.right]
    chooser = random.Random(131)
    results, totals = {}, {side: [] for side in sides}
    for q in questions:
        paths = {}
        for side in sides:
            label, kind = side.split(":")
            paths[side] = pathlib.Path(args.out, label, f"{q['id']}.{kind}.md")
        if not all(p.exists() for p in paths.values()):
            continue
        order = list(sides)
        chooser.shuffle(order)
        scored = grade(q, paths[order[0]].read_text(), paths[order[1]].read_text(), args.model)
        mapped = {order[0]: scored["A"], order[1]: scored["B"]}
        results[q["id"]] = mapped
        for side in sides:
            s = mapped[side]
            totals[side].append((s["correct"], s["complete"], s["clear"]))
        line = "  ".join(f"{side}: {mapped[side]['correct']}/{mapped[side]['complete']}/{mapped[side]['clear']}" for side in sides)
        print(f"{q['id']:<14} {line}", flush=True)
    name = "grades-" + "-vs-".join(s.replace(":", ".") for s in sides) + ".json"
    pathlib.Path(args.out, name).write_text(json.dumps(results, indent=1))
    for side in sides:
        rows = totals[side]
        if rows:
            means = [round(sum(r[i] for r in rows) / len(rows), 2) for i in range(3)]
            print(f"mean {side}: correct {means[0]} complete {means[1]} clear {means[2]}")


if __name__ == "__main__":
    sys.exit(main())
