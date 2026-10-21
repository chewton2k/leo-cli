import argparse
import http.cookiejar
import json
import os
import pathlib
import re
import subprocess
import sys
import tempfile
import time
import urllib.request

HERE = pathlib.Path(__file__).resolve().parent


def raw_answer(prompt, model, effort):
    args = ["claude", "-p", "--model", model, "--output-format", "text"]
    if effort:
        args += ["--effort", effort]
    done = subprocess.run(args, input=prompt, capture_output=True, text=True, timeout=900)
    if done.returncode != 0:
        raise RuntimeError(done.stderr.strip() or "claude failed")
    return done.stdout.strip()


class Felix:
    def __init__(self, model, effort, port):
        self.home = tempfile.mkdtemp(prefix="felix-eval-")
        os.makedirs(os.path.join(self.home, "notes"))
        lines = ["[chat]", 'chain = ["claude_code"]', "", "[providers.claude_code]", f'model = "{model}"']
        if effort:
            lines.append(f'effort = "{effort}"')
        pathlib.Path(self.home, "config.toml").write_text("\n".join(lines) + "\n")
        env = dict(os.environ, LEO_HOME=self.home, LEO_NO_OPEN="1", LEO_NO_UPDATE_CHECK="1")
        self.log = open(os.path.join(self.home, "serve.log"), "w+")
        self.server = subprocess.Popen(
            [os.environ.get("LEO", "leo"), "serve", "--local", "--port", str(port)],
            env=env, stdout=self.log, stderr=subprocess.STDOUT,
        )
        token = None
        for _ in range(100):
            time.sleep(0.2)
            self.log.seek(0)
            found = re.search(r"token=([0-9a-f]+)", self.log.read())
            if found:
                token = found.group(1)
                break
        if not token:
            self.close()
            raise RuntimeError("leo serve printed no link")
        self.base = f"http://127.0.0.1:{port}"
        jar = http.cookiejar.CookieJar()
        self.opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(jar))
        self.opener.open(f"{self.base}/?token={token}").read()

    def answer(self, prompt):
        body = json.dumps({"messages": [{"role": "user", "text": prompt}], "mode": "chat"}).encode()
        request = urllib.request.Request(
            f"{self.base}/api/chat", data=body, headers={"content-type": "application/json"}
        )
        text, steps = "", []
        with self.opener.open(request, timeout=900) as reply:
            for line in reply:
                if not line.strip():
                    continue
                event = json.loads(line)
                if "t" in event:
                    text += event["t"]
                elif event.get("restart"):
                    text = ""
                elif "step" in event:
                    steps.append(event["step"])
                elif "error" in event:
                    raise RuntimeError(event["error"])
        return text.strip(), steps

    def close(self):
        self.server.terminate()
        try:
            self.server.wait(10)
        except subprocess.TimeoutExpired:
            self.server.kill()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--label", required=True)
    parser.add_argument("--model", default="claude-sonnet-5-5")
    parser.add_argument("--effort", default="medium")
    parser.add_argument("--only", choices=["raw", "felix"])
    parser.add_argument("--ids", nargs="*")
    parser.add_argument("--out", default=os.environ.get("FELIX_EVAL_OUT", str(HERE / "out")))
    parser.add_argument("--port", type=int, default=47812)
    args = parser.parse_args()
    questions = json.loads((HERE / "questions.json").read_text())
    if args.ids:
        questions = [q for q in questions if q["id"] in args.ids]
    out = pathlib.Path(args.out, args.label)
    out.mkdir(parents=True, exist_ok=True)
    timings = {}
    felix = None if args.only == "raw" else Felix(args.model, args.effort, args.port)
    try:
        for q in questions:
            for kind in ("raw", "felix"):
                if args.only and args.only != kind:
                    continue
                started = time.time()
                try:
                    if kind == "raw":
                        text = raw_answer(q["prompt"], args.model, args.effort)
                    else:
                        text, steps = felix.answer(q["prompt"])
                        if steps:
                            text += "\n\n<!-- steps: " + " | ".join(map(str, steps)) + " -->"
                except Exception as error:
                    text = f"(failed: {error})"
                seconds = round(time.time() - started, 1)
                timings[f"{q['id']}.{kind}"] = seconds
                (out / f"{q['id']}.{kind}.md").write_text(text + "\n")
                print(f"{q['id']:<14} {kind:<6} {seconds:>6}s {len(text):>6} chars", flush=True)
    finally:
        if felix:
            felix.close()
    previous = out / "timings.json"
    merged = json.loads(previous.read_text()) if previous.exists() else {}
    merged.update(timings)
    previous.write_text(json.dumps(merged, indent=1))


if __name__ == "__main__":
    sys.exit(main())
