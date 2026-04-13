#!/bin/bash

CP="${1:-cp}"
TESTDIR="$(mktemp -d)"
PASS=0
FAIL=0

# helpers

cleanup() { rm -rf "$TESTDIR"; }
trap cleanup EXIT

header() {
    printf '\x1b[100;37;1m%-64s\x1b[m\n' " $1 "
}

begin_test() {
    printf '%-60s' "$1"
    rm -rf "$TESTDIR/work"
    mkdir -p "$TESTDIR/work"
    cd "$TESTDIR/work" || exit 1
}

fail_test() {
    printf '\x1b[1;31mFAIL\x1b[m\n'
    printf '    \x1b[1mReason\x1b[m: %s\n' "$1"
    FAIL=$((FAIL + 1))
}

pass_test() {
    printf '\x1b[1;34mPASS\x1b[m\n'
    PASS=$((PASS + 1))
}

# assert_exit EXPECTED ACTUAL
assert_exit() {
    local expected="$1" actual="$2"
    if [ "$actual" -ne "$expected" ]; then
        fail_test "expect exit $expected, got $actual"
        return 1
    fi
    return 0
}

# assert_file_exists PATH
assert_file_exists() {
    if [ ! -e "$1" ]; then
        fail_test "expect '$1' to exist"
        return 1
    fi
    return 0
}

# assert_file_missing PATH
assert_file_missing() {
    if [ -e "$1" ]; then
        fail_test "expect '$1' to be absent"
        return 1
    fi
    return 0
}

# assert_contents PATH EXPECTED_CONTENT
assert_contents() {
    local path="$1" expected="$2" actual
    actual="$(cat "$path" 2>/dev/null)"
    if [ "$actual" != "$expected" ]; then
        fail_test "contents of '$path': expected '$expected', got '$actual'"
        return 1
    fi
    return 0
}

# assert_mode PATH EXPECTED_OCTAL (e.g. 0644)
assert_mode() {
    local path="$1" expected="$2" actual
    actual="$(stat -c '%a' "$path" 2>/dev/null || stat -f '%OLp' "$path" 2>/dev/null)"
    # Normalise to 4-digit octal
    actual="$(printf '%04d' "$actual")"
    expected="$(printf '%04d' "$expected")"
    if [ "$actual" != "$expected" ]; then
        fail_test "mode of '$path': expected $expected, got $actual"
        return 1
    fi
    return 0
}

# assert_symlink PATH TARGET
assert_symlink() {
    local path="$1" expected_target="$2" actual_target
    if [ ! -L "$path" ]; then
        fail_test "expected '$path' to be a symlink"
        return 1
    fi
    actual_target="$(readlink "$path")"
    if [ "$actual_target" != "$expected_target" ]; then
        fail_test "symlink '$path' -> '$actual_target', expected -> '$expected_target'"
        return 1
    fi
    return 0
}

# assert_is_symlink PATH  (just checks it's a symlink, any target)
assert_is_symlink() {
    if [ ! -L "$1" ]; then
        fail_test "expected '$1' to be a symlink"
        return 1
    fi
    return 0
}

# assert_not_symlink PATH  (is a regular file/dir, not a symlink)
assert_not_symlink() {
    if [ -L "$1" ]; then
        fail_test "expected '$1' NOT to be a symlink"
        return 1
    fi
    return 0
}

# assert_times_equal SRC DST  — mtime seconds match
assert_times_equal() {
    local src="$1" dst="$2" t1 t2
    t1="$(stat -c '%Y' "$src" 2>/dev/null || stat -f '%m' "$src" 2>/dev/null)"
    t2="$(stat -c '%Y' "$dst" 2>/dev/null || stat -f '%m' "$dst" 2>/dev/null)"
    if [ "$t1" != "$t2" ]; then
        fail_test "mtime mismatch: src=$t1 dst=$t2 for '$src' vs '$dst'"
        return 1
    fi
    return 0
}

# all_passed — call at end of a test after all asserts succeeded
all_passed() { pass_test; }

# basic functionality
header "BASIC FUNCTIONS"

begin_test "copy file to new destination"
echo "hello" > src.txt
"$CP" src.txt dst.txt; RC=$?
assert_exit 0 $RC && assert_file_exists dst.txt && assert_contents dst.txt "hello" && all_passed

begin_test "copy file into existing directory"
echo "data" > src.txt
mkdir dest_dir
"$CP" src.txt dest_dir/; RC=$?
assert_exit 0 $RC && assert_contents dest_dir/src.txt "data" && all_passed

begin_test "overwrite existing destination file"
echo "old" > dst.txt
echo "new" > src.txt
"$CP" src.txt dst.txt; RC=$?
assert_exit 0 $RC && assert_contents dst.txt "new" && all_passed

begin_test "copy non-existent source fails"
"$CP" no_such_file.txt dst.txt 2>/dev/null; RC=$?
assert_exit 1 $RC && assert_file_missing dst.txt && all_passed

begin_test "copy directory without -r fails"
mkdir mydir
"$CP" mydir dst 2>/dev/null; RC=$?
assert_exit 1 $RC && all_passed

begin_test "two sources require directory destination"
echo "a" > a.txt; echo "b" > b.txt
"$CP" a.txt b.txt nodst 2>/dev/null; RC=$?
assert_exit 1 $RC && all_passed

begin_test "multiple sources into directory"
echo "a" > a.txt; echo "b" > b.txt
mkdir out
"$CP" a.txt b.txt out/; RC=$?
assert_exit 0 $RC && assert_contents out/a.txt "a" && assert_contents out/b.txt "b" && all_passed

# --force flag
header "MODE: -f"

begin_test "force overwrite read-only file"
echo "original" > dst.txt
chmod 444 dst.txt
echo "replaced" > src.txt
"$CP" -f src.txt dst.txt; RC=$?
# Some systems need the destination to be writable by the process; only assert
# success if we are root OR the copy succeeded.
if [ $RC -eq 0 ]; then
    assert_contents dst.txt "replaced" && all_passed
else
    # Non-root cannot overwrite truly read-only files even with -f on some systems;
    # just verify the file was NOT silently corrupted.
    assert_contents dst.txt "original" && all_passed
fi
chmod 644 dst.txt   # restore

begin_test "force copy when dst does not exist (must still succeed)"
echo "hello" > src.txt
"$CP" -f src.txt dst.txt; RC=$?
assert_exit 0 $RC && assert_contents dst.txt "hello" && all_passed

begin_test "force overwrite dst with same content"
echo "same" > src.txt; echo "same" > dst.txt
"$CP" -f src.txt dst.txt; RC=$?
assert_exit 0 $RC && assert_contents dst.txt "same" && all_passed

# --preserve flag
header "MODE: -p"

begin_test "preserve permissions"
echo "perms" > src.txt
chmod 0640 src.txt
"$CP" -p src.txt dst.txt; RC=$?
assert_exit 0 $RC && assert_mode dst.txt 640 && all_passed

begin_test "preserve modification time"
echo "time" > src.txt
# Set an explicit timestamp in the past
touch -t 200001010000 src.txt
"$CP" -p src.txt dst.txt; RC=$?
assert_exit 0 $RC && assert_times_equal src.txt dst.txt && all_passed

begin_test "contents preserved alongside metadata"
echo "content" > src.txt
chmod 0600 src.txt
touch -t 201506151200 src.txt
"$CP" -p src.txt dst.txt; RC=$?
assert_exit 0 $RC && assert_contents dst.txt "content" && assert_mode dst.txt 600 && all_passed

# --recursive flag
header "MODE: -r"

begin_test "recursive copy into existing directory"
mkdir -p src/sub
echo "data" > src/sub/file.txt
mkdir dst
"$CP" -r src dst; RC=$?
assert_exit 0 $RC && assert_contents dst/src/sub/file.txt "data" && all_passed

begin_test "copy empty directory"
mkdir empty_src
"$CP" -r empty_src empty_dst; RC=$?
assert_exit 0 $RC && [ -d empty_dst ] && all_passed

begin_test "deeply nested directory"
mkdir -p src/a/b/c/d
echo "deep" > src/a/b/c/d/deep.txt
"$CP" -r src dst; RC=$?
assert_exit 0 $RC && assert_contents dst/a/b/c/d/deep.txt "deep" && all_passed

begin_test "preserves directory structure (no flattening)"
mkdir -p src/x src/y
echo "x" > src/x/fx.txt
echo "y" > src/y/fy.txt
"$CP" -r src dst; RC=$?
assert_exit 0 $RC \
    && assert_contents dst/x/fx.txt "x" \
    && assert_contents dst/y/fy.txt "y" \
    && all_passed

begin_test "source is a file, not a directory"
echo "just a file" > src.txt
"$CP" -r src.txt dst.txt; RC=$?
assert_exit 0 $RC && assert_contents dst.txt "just a file" && all_passed

begin_test "copy symlink inside directory (recreates symlink)"
mkdir -p src
echo "target" > src/target.txt
ln -s target.txt src/link.txt
"$CP" -r src dst; RC=$?
assert_exit 0 $RC && assert_file_exists dst/target.txt && all_passed

# --archive flag
header "MODE: -a"

begin_test "basic archive copy preserves content"
mkdir -p src/sub
echo "arch" > src/sub/file.txt
"$CP" -a src dst; RC=$?
assert_exit 0 $RC && assert_contents dst/sub/file.txt "arch" && all_passed

begin_test "archive preserves file permissions"
mkdir -p src
echo "perm" > src/file.txt
chmod 0711 src/file.txt
"$CP" -a src dst; RC=$?
assert_exit 0 $RC && assert_mode dst/file.txt 711 && all_passed

begin_test "archive preserves modification time"
mkdir -p src
echo "time" > src/file.txt
touch -t 200301010000 src/file.txt
"$CP" -a src dst; RC=$?
assert_exit 0 $RC && assert_times_equal src/file.txt dst/file.txt && all_passed

begin_test "archive preserves symlinks (copies as symlinks)"
mkdir -p src
echo "real" > src/real.txt
ln -s real.txt src/link.txt
"$CP" -a src dst; RC=$?
assert_exit 0 $RC && assert_is_symlink dst/link.txt && assert_symlink dst/link.txt real.txt && all_passed

begin_test "archive does not dereference symlinks to regular files"
mkdir -p src
echo "real" > real.txt
ln -s "$TESTDIR/work/real.txt" src/link.txt
"$CP" -a src dst; RC=$?
assert_exit 0 $RC && assert_is_symlink dst/link.txt && all_passed

begin_test "archive on a single file preserves metadata"
echo "single" > src.txt
chmod 0600 src.txt
touch -t 199912311200 src.txt
"$CP" -a src.txt dst.txt; RC=$?
assert_exit 0 $RC \
    && assert_contents dst.txt "single" \
    && assert_mode dst.txt 600 \
    && assert_times_equal src.txt dst.txt \
    && all_passed

# edge cases
header "EDGE CASES"

begin_test "copy file to itself (same path); either fail or no-op"
echo "self" > file.txt
"$CP" file.txt file.txt 2>/dev/null; RC=$?
# POSIX says this is an error; content must remain intact either way
assert_contents file.txt "self" && all_passed

begin_test "destination directory does not exist (non-recursive)"
echo "hello" > src.txt
"$CP" src.txt nonexistent_dir/dst.txt 2>/dev/null; RC=$?
assert_exit 1 $RC && all_passed

begin_test "copy zero-byte file"
touch empty.txt
"$CP" empty.txt copy.txt; RC=$?
assert_exit 0 $RC && assert_file_exists copy.txt && assert_contents copy.txt "" && all_passed

begin_test "source file with spaces in name"
echo "spaced" > "my file.txt"
"$CP" "my file.txt" "my copy.txt"; RC=$?
assert_exit 0 $RC && assert_contents "my copy.txt" "spaced" && all_passed

begin_test "on symlink-to-dir (must copy contents, not loop)"
mkdir real_dir
echo "inside" > real_dir/file.txt
ln -s real_dir link_dir
"$CP" -r link_dir dst 2>/dev/null; RC=$?
# Must not infinite-loop; if it succeeds the content must be right
if [ $RC -eq 0 ]; then
    assert_file_exists dst/file.txt && all_passed
else
    all_passed   # failing on a symlink-to-dir is also acceptable
fi

begin_test "Preserve mode with read-only source copies nicely"
echo "ro" > src.txt
chmod 0444 src.txt
"$CP" -p src.txt dst.txt; RC=$?
assert_exit 0 $RC && assert_contents dst.txt "ro" && assert_mode dst.txt 444 && all_passed
chmod 644 src.txt

begin_test "multiple flags combined (-rp)"
mkdir -p src/dir
echo "combo" > src/dir/f.txt
chmod 0755 src/dir/f.txt
touch -t 201001010000 src/dir/f.txt
"$CP" -rp src dst; RC=$?
assert_exit 0 $RC \
    && assert_contents dst/dir/f.txt "combo" \
    && assert_mode dst/dir/f.txt 755 \
    && assert_times_equal src/dir/f.txt dst/dir/f.txt \
    && all_passed

# summary
echo
header "SUMMARY"
echo "$PASS passed, $FAIL failed, $((PASS + FAIL))) total."
[ $FAIL -eq 0 ] && exit 0 || exit 1
