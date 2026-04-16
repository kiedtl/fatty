test_cp:
	@cargo build --package crickhollow 2>/dev/null
	@rm -rf target/_foo target/_bar
	@mkdir target/_foo
	@mkdir target/_bar
	@dd if=/dev/zero bs=380M count=1 > target/_foo/f1.ign 2>/dev/null
	@dd if=/dev/zero bs=90K  count=1 > target/_foo/f2.ign 2>/dev/null
	@dd if=/dev/zero bs=100M count=1 > target/_foo/f3.ign 2>/dev/null
	@dd if=/dev/zero bs=10K  count=1 > target/_foo/f4.ign 2>/dev/null
	@dd if=/dev/zero bs=800M count=1 > target/_foo/f5.ign 2>/dev/null
	@target/debug/crickhollow -r target/_foo/ target/_bar/
