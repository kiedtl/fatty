# Marish

This document describes the shell syntax and semantics for Marish, a shell based
on structured data.

## Values

Integers and floats work as you'd expect.

```
0 0.0 3 4 5 3.4
```

True, false, undefined, and nil:

```
$t $f $nil $undef
```

Note: `nil` (absence of a value) is distinct from `undefined` (an incomplete,
unintelligible, or undefined value).

Array literals:

```
[1 2 3 [4 5 6]]
```

Tables:

```
[[header1 header2 header3]; [a b c] [d e f]]
```

Table headers are required, even for empty tables:

```
[[header1 header2 header3];]
```

Note the semicolon needed to distinguish this from a nested array. It is only
used after the header array.

Marish does not have a record/map type. A record is simply a table with one row.

## Strings

Strings can be unquoted, single quoted, or double quoted:

```
echo ~/test*        # -> /home/foo/test1 /home/foo/test2 /home/foo/test3
echo "test*\x1b[m"  # -> test*<ESC>[m
echo 'test*\x1b[m'  # -> test*\x1b[m
```

Unquoted strings can have escapes and globs, quoted strings can have escapes, and
unquoted strings can have neither.

Escape sequences:

- `\a`, `\b`, `\r`, `\n`, `\v`, `\f`, `\t`, `\0`
- `\x<HEX>`
- `\o<OCTAL>`

Only `*` and `**` globs are supported at this time.

## Declarations

Variables are declared with the `$var = value` syntax:

```
$var = 0
$var = "test"
$var = [1 2 3]
```

Indexing operations are supported:

```
$var = [1 2 3]
echo $var[0]        # -> 1
```

For tables, you can index by column, which uses a different syntax:

```
$var = [[a b c]; [1 2 3] [2 3 4]]
echo $var.a[0]          # -> 1
echo $var.["b"][0]      # -> 2
```

The `$var.field` syntax only allows for indexing into a column named as a
string; since tables can have columns with arbitrary names (numbers, floats,
even tables and arrays), the `$var.[expr]` syntax is required at times.

Out-of-bounds or unknown column indexing returns `nil` (applies to both arrays
and tables).

## Operators

Boolean operators use the PowerShell syntax.

```
-eq -ne -gt -lt -ge -le -and -or -xor
-lk # Like
-nk # Not like
-not # The only unary operator
```

Arithmetic operators are as normal.

```
+ - * /
```

## Commands

Syntax:

```
command "arg" 2 4 [1 2 3] $var.field
```

Note that `command` must be an unquoted string. Variables, quoted strings, and
the like are not permitted. The `command` must be resolvable at compile time.

TODO: syntax to execute a string or variable, i.e. `do "command" arg1 arg2`

## Pipelines

Commands are always given access to stdin, stdout/err, and two new FDs: Fd3 and
Fd4, aka object out and object in.

Stdin and stdout/err are used as normal. Fd3 and Fd4 are unique to Marish/Fatty
and are used to transmit structured data. Only commands that can take advantage
of this will use Fd3/4.

Example:

```
fatty_ls | where $_.size -lt 35000 | tee -a file
```

`fatty_ls` transmits a table through fd3 to `where`, and transmits nothing over
stdout. `where` filters object out based on the predicate, transmitting it to
the next stage in the pipeline; it also transmits nothing over stdout. `tee`, on
the other hand, is the GNU coreutils utility, which has no understanding of
object out, and thus does not read from it. Marish does not redirect stdout to
object out in any case.

## Pipeline Builtins

There are some builtins that can be used in pipelines:

```
ls | where $_.size -gt 1000
```

The `where` builtin accepts a table or array and filters based on the provided
predicate, setting `$_` to the value of the current element. If it resolves to
true, the value is passed through as-is; if it resolves to a non-boolean value,
it is an immediate error. Values are collated into a table or array of the same
form as it originally was.

## Tests

Marish has a builtin unit test framework. Tests are defined with a test
declaration:

```
test "value" (
    assert $t
)
```

Currently there is only the `assert` builtin.

Running Marish with the `-t` flag causes it to run all registered test, printing
the results to stdout.

TODO: ways to run a test from within Marish, ways to run specific tests, group
tests within test suites.
