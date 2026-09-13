PREFIX ?= $(HOME)/.local
BINDIR ?= $(PREFIX)/bin
DESTDIR ?=

CARGO ?= cargo
INSTALL ?= install

BIN := qust
RELEASE_BIN := target/release/$(BIN)
INSTALL_BIN := $(DESTDIR)$(BINDIR)/$(BIN)

DESKTOP := $(BIN).desktop
DESKTOP_TEMPLATE := $(DESKTOP).in
APPDIR := $(DESTDIR)$(PREFIX)/share/applications
INSTALL_DESKTOP := $(APPDIR)/$(DESKTOP)

.PHONY: all build install uninstall clean check-release default-browser

all: build

build:
	$(CARGO) build --release

install: check-release
	$(INSTALL) -d "$(DESTDIR)$(BINDIR)"
	$(INSTALL) -m 0755 "$(RELEASE_BIN)" "$(INSTALL_BIN)"
	$(INSTALL) -d "$(APPDIR)"
	sed 's|@BIN@|$(BINDIR)/$(BIN)|' "$(DESKTOP_TEMPLATE)" > "$(INSTALL_DESKTOP)"
	chmod 0644 "$(INSTALL_DESKTOP)"
	-update-desktop-database "$(APPDIR)"

# $$BROWSER overrides xdg-settings, so it is cleared for the call only.
default-browser: install
	env -u BROWSER xdg-settings set default-web-browser "$(DESKTOP)"
	xdg-mime default "$(DESKTOP)" text/html x-scheme-handler/http x-scheme-handler/https
	@echo "qust is the default browser. Unset or update BROWSER in your shell to match."

check-release:
	@test -x "$(RELEASE_BIN)" || { \
		echo "Missing $(RELEASE_BIN). Run 'make build' first."; \
		exit 1; \
	}

uninstall:
	rm -f "$(INSTALL_BIN)" "$(INSTALL_DESKTOP)"
	-update-desktop-database "$(APPDIR)"

clean:
	$(CARGO) clean
