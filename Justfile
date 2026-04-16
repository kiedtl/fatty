test_cp:
	@cargo build --package crickhollow 2>/dev/null
	@rm -rf target/_foo target/_bar
	@mkdir target/_foo
	@mkdir target/_bar
	@dd if=/dev/zero bs=80M count=1 > target/_foo/f1.ign 2>/dev/null
	@dd if=/dev/zero bs=10M count=1 > target/_foo/f2.ign 2>/dev/null
	@dd if=/dev/zero bs=10M count=1 > target/_foo/f3.ign 2>/dev/null
	@target/debug/crickhollow -r target/_foo/ target/_bar/
