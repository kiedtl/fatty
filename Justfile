install:
	mkdir -p ~/.local/bin/fatty_bin/tools/
	mkdir -p ~/.local/bin/fatty_bin/crickhollow/
	cargo build --release -p fatty
	cargo build --release -p crickhollow
	cp target/release/fatty ~/.local/bin/
	cp target/release/select ~/.local/bin/fatty_bin/tools/
	cp target/release/crickhollow ~/.local/bin/fatty_bin/crickhollow/
	ln -fs ~/.local/bin/fatty_bin/crickhollow/crickhollow ~/.local/bin/fatty_bin/crickhollow/ps
	ln -fs ~/.local/bin/fatty_bin/crickhollow/crickhollow ~/.local/bin/fatty_bin/crickhollow/du
	ln -fs ~/.local/bin/fatty_bin/crickhollow/crickhollow ~/.local/bin/fatty_bin/crickhollow/df
	ln -fs ~/.local/bin/fatty_bin/crickhollow/crickhollow ~/.local/bin/fatty_bin/crickhollow/cp
	ln -fs ~/.local/bin/fatty_bin/crickhollow/crickhollow ~/.local/bin/fatty_bin/crickhollow/max
	chmod +x ~/.local/bin/fatty_bin/crickhollow/*
	chmod +x ~/.local/bin/fatty_bin/tools/*

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
