# noahgift content audits
#
# Build-time falsifiers (per spec 012-site-audit-contract): render each link
# in real headless Chrome via probar and assert content shape, not just HTTP
# status — the same class of check that catches Duke-scholars / Google
# Scholar style 200-but-broken pages.

AUDIT_DIR := tests/course-audit
FIXTURES  := tests/fixtures
AUDIT_BIN := $(AUDIT_DIR)/target/release/audit-courses

.PHONY: audit-build
audit-build: $(AUDIT_BIN)

$(AUDIT_BIN): $(AUDIT_DIR)/Cargo.toml $(AUDIT_DIR)/src/main.rs
	cd $(AUDIT_DIR) && cargo build --release

# Audit every outbound link from noahgift.com (faculty pages, partner pages,
# books, social, specializations). This is the spec-012 surface.
.PHONY: audit-site
audit-site: audit-build
	cd $(AUDIT_DIR) && ./target/release/audit-courses ../../$(FIXTURES)/expected-noahgift-site.md

# Audit every Coursera URL in the README's canonical course list.
.PHONY: audit-courses
audit-courses: audit-build
	cd $(AUDIT_DIR) && ./target/release/audit-courses ../../$(FIXTURES)/expected-courses.md

# Coverage gap check: pull Noah's instructor-page slugs (SSR-visible portion)
# from coursera.org and report any that aren't in the fixture. Uses curl, not
# probar, since this is a one-shot diff.
.PHONY: audit-instructor-coverage
audit-instructor-coverage:
	@curl -sL --max-time 30 \
	  -A "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 Chrome/120.0.0.0" \
	  https://www.coursera.org/instructor/noahgift \
	  | grep -oE 'href="/(learn|projects|specializations)/[a-zA-Z0-9-]+' \
	  | sed 's|href="/||' | awk -F/ '{print $$2}' | sort -u > /tmp/noahgift-instructor-slugs.txt
	@grep -oE 'https://www\.coursera\.org/(learn|projects|specializations)/[a-zA-Z0-9-]+' \
	  $(FIXTURES)/expected-courses.md \
	  | awk -F/ '{print $$NF}' | sort -u > /tmp/noahgift-fixture-slugs.txt
	@echo "Instructor SSR slugs: $$(wc -l < /tmp/noahgift-instructor-slugs.txt)"
	@echo "Fixture slugs:        $$(wc -l < /tmp/noahgift-fixture-slugs.txt)"
	@missing=$$(comm -23 /tmp/noahgift-instructor-slugs.txt /tmp/noahgift-fixture-slugs.txt); \
	if [ -n "$$missing" ]; then \
	  echo "MISSING from fixture (instructor page lists, README does not):"; \
	  echo "$$missing"; exit 1; \
	else \
	  echo "OK: every SSR-visible instructor course is in the fixture."; \
	fi

# Run everything. Site audit is the spec-012 P0 contract; courses audit is
# the README link verifier; coverage is the gap check.
.PHONY: audit-all
audit-all: audit-site audit-courses audit-instructor-coverage
	@echo "All audits passed."

.PHONY: audit-clean
audit-clean:
	rm -f $(AUDIT_DIR)/*-report.json $(AUDIT_DIR)/*.log
	cd $(AUDIT_DIR) && cargo clean
