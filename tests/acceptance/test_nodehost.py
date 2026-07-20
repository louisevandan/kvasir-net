#!/usr/bin/env python3
"""linkcpp node-host UI acceptance tests (headless Playwright).
  LINKCPP_NODEHOST_URL=http://node-host:9100 python test_nodehost.py
"""
import os, re, sys
from playwright.sync_api import sync_playwright, expect

URL = os.environ.get("LINKCPP_NODEHOST_URL", "http://node-host:9100")
results = []


def check(name, fn):
    try:
        fn(); results.append(True); print(f"PASS  {name}")
    except Exception as e:
        results.append(False); print(f"FAIL  {name}: {str(e).splitlines()[0]}")


def main():
    with sync_playwright() as p:
        page = p.chromium.launch().new_page()
        page.set_default_timeout(20000)
        page.on("dialog", lambda d: (print("DIALOG:", d.message), d.dismiss()))
        page.goto(URL)

        check("NH-1 GPU cards listed",
              lambda: (expect(page.get_by_test_id("gpu-cards")).to_contain_text("RTX 4080"),
                       expect(page.get_by_test_id("gpu-cards")).to_contain_text("RTX 3090")))

        check("NH-2 capacity shown (n/5)",
              lambda: expect(page.get_by_test_id("capacity")).to_contain_text(re.compile(r"\d+/5")))

        def nh3():
            page.wait_for_selector('[data-testid="create-btn"]')
            before = page.get_by_test_id("node-row").count()
            page.get_by_test_id("create-btn").first.click()   # create on the first GPU
            expect(page.get_by_test_id("node-row")).to_have_count(before + 1, timeout=20000)
        check("NH-3 create node adds a row", nh3)

        check("NH-4 node shows controller address",
              lambda: expect(page.get_by_test_id("node-address").first).to_contain_text(":"))

        page.screenshot(path="/t/nodehost.png", full_page=True)
    ok = sum(results)
    print(f"\n{ok}/{len(results)} node-host scenarios passed")
    sys.exit(0 if ok == len(results) else 1)


if __name__ == "__main__":
    main()
