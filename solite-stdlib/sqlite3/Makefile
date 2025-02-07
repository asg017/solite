
sqlite3: sqlite3-extra.c ../../dist/release/libsolite_stdlib.a
	gcc \
	-DSQLITE_EXTRA_INIT=core_init \
	-DHAVE_READLINE=1 \
	-lreadline \
	-I./ -I./sqlite \
	sqlite3-extra.c shell.c \
	../../dist/release/libsolite_stdlib.a \
  -Wl,-undefined,dynamic_lookup \
	-framework CoreFoundation -framework Security \
	-O3 \
	-o $@

sqlite3-extra.c: sqlite3.c core_init.c
	cat sqlite3.c core_init.c > $@
