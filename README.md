# Rusty Lambda

Rust implementation of [Lambda Calculus](https://en.wikipedia.org/wiki/Lambda_calculus)

## The Language

See the [prelude](src/prelude.txt) for a list of all built-in functions and example code.

```
; Define a Lambda
ident = \x.x

; Specify multiple parameters
f1 = \x y z.(x y)
f2 = \x.\y z.(x y)

; Evaluate from left-to-right
f3 = \x y z.(((x y) z) z)
f4 = \x y z.(x y z z)

; Numbers are converted to Church numerals
0         ; \f.\x.x
2         ; \f.\x.(f (f x))
\g.(g 5)  ; \g.(g \f.\x.(f (f (f (f (f x))))))

; Lists are also built-in
[]       ; Empty list (false)
[1]      ; (pair 1 false)
[5 2 1]  ; (pair 5 (pair 2 (pair 1 false)))

; Prelude defines some built-in functions
false
true
and
or
not

; Evaluate an expression and print the result
(succ 18)
(and (or false true) true false)
(mul 5 (add 10 1))
(map (+ 5) [2 4 6])
```

Comments begin with a semicolon `;` and continue to the end of the line.

Identifiers are any valid string of ASCII or unicode characters, excluding a few special characters (`\`, `.`, `;`, `(`, `)`, `[`, `]`, `{`, `}`) and whitespace. An identifier can be at most 65535 bytes long.

Evaluations must be enclosed in parentheses `(` `)`, or else the parser interprets it as expressions you wish to print.

```
; Right
\x y.(x y)

; Wrong!
\x y.x y
```

The code file is interpreted as a sequence of either assignments (`identifier = expression`) or expressions. Assignments are lazily evaluated, whereas expressions are evaluated one-at-a-time and printed to the console. In repl mode, the interpreter expects you to only type in one of either `identifier = expression` or `expression`.

<br />

## Basic Usage

**Interactive REPL:**

```
lambda
```

**Run code files:**

```bash
# One file
lambda code.txt

# Multiple files, executed in order
lambda code-1.txt code-2.txt
```

**Run files, then enter interactive REPL:**

```bash
lambda --interactive code.txt
# or
lambda -i code.txt code-2.txt
```

**Print each step-by-step substitution:**

```bash
lambda --steps code.txt
# or
lambda -s code.txt
```

Step `0` is the fully expanded starting expression. Steps are printed to stderr so you can still pipe output to a file.

## Encoding

The program has built-in utilities to convert to-and-from [Binary Lambda Calculus](https://esolangs.org/wiki/Binary_lambda_calculus).

```
; code.txt
test = \n.\f x.(f (n f x))
test2 = (test 2)
```

You must specify a `--term` flag, which is a lambda statement to evaluate. (Like `test` or `(test (\x.x 3))`).

**Encode to ASCII binary:**

```bash
lambda encode code.txt --term test

# 000000011100101111011010
```

**Encode to raw bytes:**

```bash
lambda encode --binary code.txt --term test

# Returns non-printable bytes:
# 01 cb da
```

_Note: any trailing bits are set to 0_

**Evaluate the term before encoding:**

```bash
lambda encode code.txt --term test2 --evaluate

# Note: evaluated "(test 2)" into "\f.\x.(f (f (f x)))" before returning
# 000001110011100111010
```

If evaluating the term, you can optionally pass the `--steps` / `-s` flag to print the reduction steps to stderr.

**Specify custom strings for `0` and `1`:**

When not using the `--binary` flag. You can specify only one flag or both flags. (_Notice we're using a lambda expression here, not just a named term._)

```bash
lambda encode code.txt --term '(test 2)' --zero a --one b
# abaaaaaaabbbaababbbbabbabaaaaaabbbaabbbaba
```

**Encode a term from the prelude:**

No need to read a code file if you only care about prelude terms.

```bash
lambda encode --term true
# 0000110
```

**Easter egg: encode as zero-width unicode characters**

Fun way to hide lambda statements in other text files.

- `0` is encoded as `\u{ffa0}` (Halfwidth Hangul Filler)
- `1` is encoded as `\u{3164}` (Hangul Filler)

```bash
lambda encode --zero-width code.txt --term test
# Raw bytes:
# efbea0efbea0efbea0efbea0efbea0efbea0efbea0e385a4e385a4e385a4
# efbea0efbea0e385a4efbea0e385a4e385a4e385a4e385a4efbea0e385a4
# e385a4efbea0e385a4efbea0
```

## Decoding

Decoding is either by ASCII characters (the default) or raw bytes (with `--binary` flag). In ASCII mode, characters that don't match `0` or `1` (or whatever you specify with `--zero` / `--one` / `--zero-width`) are ignored.

The decoded output is printed to the terminal and is valid source code that can be run by the interpreter.

**Decode a text file:**

```
000001110011100111010
```

```bash
lambda decode encoded.txt
# \x1.\x2.(x1 (x1 (x1 x2)))
```

**Decode a raw binary file:**

```
01 cb da
```

_(Raw bytes, not ASCII text)_

```bash
lambda decode --binary encoded.bin
# \x1.\x2.(x1 (x1 (x1 x2)))
```

**Evaluate the expression after decoding:**

```bash
lambda decode encoded.txt --evaluate
```

If evaluating the term, you can optionally pass the `--steps` / `-s` flag to print the reduction steps to stderr.

**Specify custom strings for `0` and `1`:**

(when not using the `--binary` flag)

```
abaaaaaaabbbaababbbbabbabaaaaaabbbaabbbaba
```

```bash
lambda decode encoded.txt --zero a --one b
# (\x1.\x2.\x3.(x2 ((x1 x2) x3)) \x1.\x2.(x1 (x1 x2)))
```

**Easter egg: encode zero-width unicode characters**

Fun way to hide lambda statements in other text files.

- `\u{ffa0}` is decoded as `0` (Halfwidth Hangul Filler)
- `\u{3164}` is decoded as `1` (Hangul Filler)

```bash
lambda decode --zero-width some-file.txt
```

<br />

## Some Technical Notes

All Lambda expressions are allocated in an [Arena Allocator](https://en.wikipedia.org/wiki/Region-based_memory_management), meaning substitution is as simple as copying references around. There are two scopes of arena allocators:

- **Assignment** - Stores all `identifier = expressions` loaded from code files. These persist for the entire duration of the program.
- **Eval** - Only exist for an `expression` and are then deallocated. We don't want temporary lambda evaluations to continually leak memory.

Lambda expressions utilize a quirk of modern 64-bit pointers where the top 16-bits are always set to 0. This allows us to store extra data in these bits. Namely:

- If the highest bit is 1, then the pointer is a term `1xxx xxxx xxxx xxxx`, where `x` is the 63-bit [de Bruijn index](https://en.wikipedia.org/wiki/De_Bruijn_index). An arena allocation isn't required in this case.
- Normal pointers are represented as `0000 yyyy yyyy yyyy`, where `y` is the 48-bit Rust reference to the expression in the arena allocator.

Lambda expressions are represented as 16-byte expressions (`left` and `right`, two 64-bit integers), where:

- **Term** - We never store this in the arena allocator because a 63-bit [de Bruijn index](https://en.wikipedia.org/wiki/De_Bruijn_index) is large enough for all practical purposes.
- **Lambda** - `left` is an expression reference (term or pointer), and `right` is a compact `&str`. The top 16-bits store the string length (1 to 32767) and the bottom 48-bits store the pointer to the `[u8]` slice. The highest bit is set to 0, or otherwise it would be a term.
- **Eval** - Both `left` and `right` are expression references (terms or pointers).

The flow for destructuring this data type is:

```rust
if (top-bit of right == 1) {
  // Right is a term, so this is an eval statement
} else if (top 16-bits of right == 0) {
  // Right is a pointer, so this is a eval statement
} else {
  // Right is a compact string, so this is a lambda statement
}
```

The code makes extensive use of the [Visitor Pattern](https://en.wikipedia.org/wiki/Visitor_pattern) to simplify this destructuring code.

The evaluation algorithm is based on [this lecture](https://www.cs.cornell.edu/courses/cs4110/2014fa/lectures/lecture15.pdf) from Cornell University. Due to the pointer logic above, the code's [de Bruijn indexes](https://en.wikipedia.org/wiki/De_Bruijn_index) start at 1 rather than 0, but otherwise the logic is the same.
