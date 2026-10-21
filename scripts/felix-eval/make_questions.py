import json

intro = """Homework 2. Naive parsing of context-free grammars

Background. In this assignment you will write a matcher generator and a parser generator for context-free grammars in OCaml. A grammar is a pair of a start symbol and a production function. Terminal symbols are strings; nonterminals are values of a user-defined variant type. We represent a symbol as follows:

type ('nonterminal, 'terminal) symbol =
  | N of 'nonterminal
  | T of 'terminal

A rule list grammar (the style from Homework 1) is a pair (start, rules) where rules is a list of (lhs, rhs) pairs and rhs is a list of symbols. A production function grammar is a pair (start, prod) where prod maps a nonterminal to its list of alternative right hand sides, in order. The order of alternatives matters: your matcher must try them left to right and must not reorder them.

Definitions. A fragment is a list of terminals, e.g. ["3"; "+"; "4"; "-"]. An acceptor is a function that takes a fragment (a suffix) and returns an option: Some x for any x means accept, None means reject. A matcher for a grammar tries to find a prefix of a fragment that is derived from the start symbol, then calls the acceptor on the remaining suffix. If the acceptor rejects, the matcher backtracks and tries the next prefix in the order induced by the grammar's alternatives. It returns whatever the first accepting call of the acceptor returns, or None if no prefix works.

Here is an example grammar for simple arithmetic:

type awksub_nonterminals = Expr | Term | Lvalue | Incrop | Binop | Num

let awkish_grammar =
  (Expr,
   function
     | Expr ->
         [[N Term; N Binop; N Expr];
          [N Term]]
     | Term ->
         [[N Num];
          [N Lvalue];
          [N Incrop; N Lvalue];
          [N Lvalue; N Incrop];
          [T"("; N Expr; T")"]]
     | Lvalue ->
         [[T"$"; N Expr]]
     | Incrop ->
         [[T"++"];
          [T"--"]]
     | Binop ->
         [[T"+"];
          [T"-"]]
     | Num ->
         [[T"0"]; [T"1"]; [T"2"]; [T"3"]; [T"4"];
          [T"5"]; [T"6"]; [T"7"]; [T"8"]; [T"9"]])

Rules you must follow. Do not use imperative features (no refs, no arrays, no mutable records, no loops). You may use the List and Stdlib modules but nothing else. Your code must not loop forever on the grammars in the test cases, but you may assume the grammar is not left recursive in a way that would make a naive top-down matcher diverge. Use only pure functions and pattern matching. Keep helper functions small and give them clear names; graders read your code.

Grading. Most of the grade is from hidden test cases similar to the public ones below. Code that does not compile gets no credit. Write your own test cases in a separate file named hw2test.ml; at least one test must use a grammar of your own that is different from the example grammars and exercises backtracking.

Public test cases (each should evaluate to true):
"""

tests = []
for i in range(1, 41):
    a, b = i % 10, (i * 7) % 10
    tests.append(f'let test_{i} = ((make_matcher awkish_grammar accept_all ["{a}"; "+"; "{b}"; "-"; "{(a+b)%10}"]) = Some [])')
    tests.append(f'let test_{i}b = ((make_matcher awkish_grammar accept_empty_suffix ["$"; "{a}"; "++"; "+"; "{b}"]) = Some [])')

outro = """

Notes on the acceptors used above:

let accept_all string = Some string
let accept_empty_suffix = function
   | _::_ -> None
   | x -> Some x

Submission. Submit hw2.ml and hw2test.ml, plus a short after-action report (plain text, a few paragraphs) describing weaknesses of your solution and why you chose your design.

---

My question (this is the part I actually need help with): Problem 1 says to write a function convert_grammar gram1 that returns a Homework 2 style grammar (a production function) that is equivalent to the Homework 1 style grammar gram1 (a rule list). Write convert_grammar in OCaml following all the rules above, explain how it works, and show what it returns for the start symbol Expr when given the awkish rules as a rule list."""

long_paste = intro + "\n".join(tests) + outro
more = [t.replace("let test_", "let extra_") for t in tests] * 2
big_paste = intro + "\n".join(tests + more) + outro

questions = [
    {"id": "ocaml-infer", "prompt": "CS131 practice: what type does OCaml infer for `let rec f g = function [] -> [] | x :: xs -> g x :: f g xs`? Walk through the inference.", "must": ["('a -> 'b) -> 'a list -> 'b list", "f is map", "unify g's argument with list element type", "result list type from g's return type", "explains recursion forces f's type consistent"]},
    {"id": "curry", "prompt": "For my programming languages class: explain currying vs partial application in OCaml, and why `let add x y = x + y` has type int -> int -> int. Is `(+) 3` a curried function?", "must": ["-> is right associative so int -> (int -> int)", "currying means multi-arg as chain of single-arg functions", "partial application supplies fewer args and returns a function", "(+) 3 is a partial application of curried (+) giving int -> int", "small example"]},
    {"id": "prolog-unify", "prompt": "Prolog homework question: what does the query `?- append(X, [c|Y], [a,b,c,d]).` return, and in what order? Explain how backtracking produces the answers.", "must": ["X = [a,b], Y = [d]", "only one solution", "append clauses tried in order: base then recursive", "unification of [c|Y] with remaining list fails until c", "backtracking explained step by step"]},
    {"id": "callcc", "prompt": "Trace this Scheme and give the value: `(+ 1 (call/cc (lambda (k) (+ 10 (k 5)))))`. Then what if I save k in a global and call it later with 20?", "must": ["result 6", "k is the continuation (+ 1 [])", "calling k abandons (+ 10 ...)", "later (k 20) returns 21 to top level context", "explains escaping continuation"]},
    {"id": "ambiguity", "prompt": "Is this grammar ambiguous? E -> E - E | num. Show two parse trees for 1 - 2 - 3 if so, explain why it matters for evaluation, and rewrite it to be unambiguous and left-associative.", "must": ["yes ambiguous", "two trees (1-2)-3 and 1-(2-3)", "values -4 and 2", "E -> E - num | num", "explains left recursion gives left associativity"]},
    {"id": "java-volatile", "prompt": "CS131 Java concurrency: in the Java Memory Model, what does `volatile` guarantee and not guarantee? Is `count++` on a volatile int thread-safe? What should I use instead?", "must": ["visibility and happens-before on write/read", "prevents certain reorderings", "no atomicity for read-modify-write", "count++ not thread safe", "AtomicInteger or synchronized"]},
    {"id": "python-closure", "prompt": "Why does `fs = [lambda: i for i in range(3)]; print([f() for f in fs])` print [2, 2, 2] in Python? How do I fix it, and how does this relate to static scoping?", "must": ["closures capture variable not value", "late binding: i looked up at call time", "fix with default arg lambda i=i: i or functools.partial", "Python is lexically scoped; the binding is shared", "correct output after fix [0,1,2]"]},
    {"id": "scoping", "prompt": "Given `x = 1; def f(): return x; def g(): x = 2; return f()` in a language with this syntax, what does g() return under static scoping vs dynamic scoping? Explain how each resolves x.", "must": ["static: 1", "dynamic: 2", "static uses definition environment", "dynamic uses call stack most recent binding", "mentions which real languages use which"]},
    {"id": "tail-rec", "prompt": "Write a tail-recursive OCaml function to sum a list of ints and explain why it's tail recursive and why that matters. Compare it to the naive version.", "must": ["accumulator helper", "recursive call in tail position", "constant stack space / no stack overflow", "naive version does work after recursive call", "code compiles"]},
    {"id": "long-paste", "prompt": long_paste, "must": ["convert_grammar returns (start, fun nt -> list of rhs for nt in order)", "uses List.filter/List.map or fold over the rule list", "keeps rule order", "no imperative features", "shows Expr maps to [[N Term; N Binop; N Expr]; [N Term]]"]},
 {"id": "big-paste", "prompt": big_paste, "must": ["convert_grammar returns (start, fun nt -> list of rhs for nt in order)", "uses List.filter/List.map or fold over the rule list", "keeps rule order", "no imperative features", "shows Expr maps to [[N Term; N Binop; N Expr]; [N Term]]"]},
]
json.dump(questions, open("questions.json", "w"), indent=1)
print(len(long_paste), len(big_paste))
