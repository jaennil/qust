PREFIX ?= $(HOME)/.local
BINDIR ?= $(PREFIX)/bin
DESTDIR ?=

CARGO ?= cargo
INSTALL ?= install

BIN := qust
RELEASE_BIN := target/release/$(BIN)
INSTALL_BIN := $(DESTDIR)$(BINDIR)/$(BIN)

.PHONY: all build install uninstall clean check-release

all: build

build:
	$(CARGO) build --release

install: check-release
	$(INSTALL) -d "$(DESTDIR)$(BINDIR)"
	$(INSTALL) -m 0755 "$(RELEASE_BIN)" "$(INSTALL_BIN)"

check-release:
	@test -x "$(RELEASE_BIN)" || { \
		echo "Missing $(RELEASE_BIN). Run 'make build' first."; \
		exit 1; \
	}

uninstall:
	rm -f "$(INSTALL_BIN)"

clean:
	$(CARGO) clean
