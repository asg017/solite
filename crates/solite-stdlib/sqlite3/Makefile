
CARGO_TARGET=debug

SQLITE_VERSION=3490000
SQLITE_YEAR=2025

BUILD_DIR=dist/$(CARGO_TARGET)
AMALGAMATION_DIR=$(BUILD_DIR)/sqlite-amalgamation-$(SQLITE_VERSION)
SQLITE3_C=$(AMALGAMATION_DIR)/sqlite3.c
SHELL_C=$(AMALGAMATION_DIR)/shell.c

ifeq ($(shell uname -s),Darwin)
CONFIG_DARWIN=y

CFLAGS += -Wl,-undefined,dynamic_lookup
# macos readline support
CFLAGS += -DHAVE_READLINE=1 -lreadline 
# ?
CFLAGS += -framework CoreFoundation 
# ?
CFLAGS += -framework Security 
# arboard/clipboard
CFLAGS += -framework AppKit

else ifeq ($(OS),Windows_NT)
CONFIG_WINDOWS=y
else
CONFIG_LINUX=y
CFLAGS += -lssl -lcrypto -lm
endif


$(BUILD_DIR):
	mkdir -p $@

$(BUILD_DIR)/sqlite-amalgamation-$(SQLITE_VERSION).zip: $(BUILD_DIR)
	curl -o $@ 'https://www.sqlite.org/$(SQLITE_YEAR)/sqlite-amalgamation-$(SQLITE_VERSION).zip'

$(AMALGAMATION_DIR): $(BUILD_DIR)/sqlite-amalgamation-$(SQLITE_VERSION).zip
	rm -rf $(AMALGAMATION_DIR)
	unzip -d $(BUILD_DIR) $<
	touch $@

$(SQLITE3_C) $(SHELL_C): $(AMALGAMATION_DIR)

$(BUILD_DIR)/sqlite3: core_init.c ../../target/$(CARGO_TARGET)/libsolite_stdlib.a $(SQLITE3_C) $(SHELL_C)
	gcc \
	-DSQLITE_EXTRA_INIT=core_init \
	-DSQLITE_CORE \
	-I./ -I$(AMALGAMATION_DIR) \
	$^ \
	$(CFLAGS) \
	-O3 \
	-o $@

.PHONY: all clean
all: $(BUILD_DIR)/sqlite3
clean:
	rm -rf $(BUILD_DIR)