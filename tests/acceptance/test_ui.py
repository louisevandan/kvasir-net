#!/usr/bin/env python3
"""linkcpp web UI acceptance tests (headless Playwright).

Runs against the live controller UI. Uses stable test hooks (data-testid) per the
headless-browser markup contract. Exits non-zero if any scenario fails.

  LINKCPP_URL=http://controller:9000 python test_ui.py
"""
import os, sys
from playwright.sync_api import sync_playwright, expect

URL = os.environ.get("LINKCPP_URL", "http://controller:9000")
results = []


def check(name, fn):
    try:
        fn()
        results.append((name, True, ""))
        print(f"PASS  {name}")
    except Exception as e:
        results.append((name, False, str(e).splitlines()[0]))
        print(f"FAIL  {name}: {str(e).splitlines()[0]}")


def main():
    with sync_playwright() as p:
        browser = p.chromium.launch()
        page = browser.new_page()
        page.set_default_timeout(20000)
        page.goto(URL)

        # AT-1: nodes registered, resources visible
        def at1():
            t = page.get_by_test_id("nodes-table")
            expect(t).to_contain_text("RTX 4080")
            expect(t).to_contain_text("RTX 3090")
        check("AT-1 nodes show GPU + resources", at1)

        # AT-2: models listed
        def at2():
            expect(page.get_by_test_id("models-list")).to_contain_text("OLMoE")
        check("AT-2 models listed", at2)

        # AT-3: plan feasible
        def at3():
            page.get_by_test_id("serve-model").select_option("OLMoE-1B-7B-0924-Instruct-Q4_K_M.gguf")
            page.get_by_test_id("serve-ctx").fill("4096")
            page.get_by_test_id("serve-parallel").fill("1")
            page.get_by_test_id("plan-btn").click()
            expect(page.get_by_test_id("plan-verdict")).to_contain_text("FEASIBLE", timeout=20000)
        check("AT-3 plan feasible", at3)

        # AT-3b: infeasible on absurd parallelism
        def at3b():
            page.get_by_test_id("serve-model").select_option("Qwen2.5-32B-Instruct-Q4_K_M.gguf")
            page.get_by_test_id("serve-ctx").fill("8192")
            page.get_by_test_id("serve-parallel").fill("64")
            page.get_by_test_id("plan-btn").click()
            expect(page.get_by_test_id("plan-verdict")).to_contain_text("INFEASIBLE", timeout=20000)
        check("AT-3b plan infeasible + reason", at3b)

        # AT-4: a model is being served (started earlier via API)
        def at4():
            expect(page.get_by_test_id("serve-status")).to_contain_text("running", timeout=20000)
        check("AT-4 serving status running", at4)

        # AT-5: chat end-to-end through the distributed cluster
        def at5():
            page.get_by_test_id("chat-input").fill("Name the primary colors.")
            page.get_by_test_id("chat-send").click()
            # wait until the loading placeholder is replaced by a real completion
            page.wait_for_function(
                "() => { const t = document.querySelector('[data-testid=chat-output]')"
                ".innerText.trim(); return t.length > 3 && t !== '\\u2026'; }",
                timeout=40000)
            txt = page.get_by_test_id("chat-output").inner_text().strip()
            assert "chat error" not in txt, txt
            assert len(txt) > 3, f"empty/short chat output: {txt!r}"
        check("AT-5 chat returns completion", at5)

        try:
            page.screenshot(path="/t/ui.png", full_page=True)
        except Exception:
            pass
        browser.close()

    ok = sum(1 for _, p_, _ in results if p_)
    print(f"\n{ok}/{len(results)} acceptance scenarios passed")
    sys.exit(0 if ok == len(results) else 1)


if __name__ == "__main__":
    main()
