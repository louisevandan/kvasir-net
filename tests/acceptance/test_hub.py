#!/usr/bin/env python3
"""linkcpp hub UI acceptance (headless Playwright) — left-menu Nodes + Controllers.
  LINKCPP_HUB_URL=http://host.docker.internal:19000 python test_hub.py
"""
import os, re, sys
from playwright.sync_api import sync_playwright, expect

URL = os.environ.get("LINKCPP_HUB_URL", "http://host.docker.internal:19000")
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
        page.on("dialog", lambda d: d.accept())
        page.goto(URL)

        check("HUB-1 sidebar sections present", lambda: (
            expect(page.get_by_test_id("add-ctrl-btn")).to_be_visible(),
            expect(page.get_by_test_id("firewall-menu")).to_be_visible(),
            expect(page.get_by_test_id("node-list")).to_be_visible(),
            expect(page.get_by_test_id("ctrl-list")).to_be_visible()))

        def configure_node_slot():
            expect(page.locator('[data-testid="node-item"][data-kind="local"]')).to_have_count(5, timeout=20000)
            first = page.locator('[data-testid="node-item"][data-kind="local"]').first
            first.click()
            expect(page.get_by_test_id("node-config")).to_be_visible()
            page.wait_for_function("()=>{const s=document.querySelector('[data-testid=node-gpu]');return s&&s.options.length>0}")
            expect(page.get_by_test_id("node-vram-slider")).to_be_visible()
            expect(page.get_by_test_id("node-ram-slider")).to_be_visible()
            expect(page.get_by_test_id("node-cores-slider")).to_be_visible()
            page.get_by_test_id("node-name-edit").click()
            page.get_by_test_id("node-name").fill("acceptance-node")
            page.get_by_test_id("node-save-btn").click()
            expect(page.get_by_test_id("node-item").filter(has_text="acceptance-node")).to_be_visible(timeout=20000)
        check("HUB-2 configure a fixed node slot", configure_node_slot)

        def node_detail():
            page.get_by_test_id("node-item").last.click()
            expect(page.get_by_test_id("node-detail")).to_be_visible()
            expect(page.get_by_test_id("node-vram-bar")).to_be_visible()
            expect(page.get_by_test_id("node-log")).to_be_visible()
        check("HUB-3 node detail shows resources + log", node_detail)

        def add_ctrl():
            before = page.get_by_test_id("ctrl-item").count()
            page.get_by_test_id("add-ctrl-btn").click()
            page.get_by_test_id("ctrl-create-btn").click()
            expect(page.get_by_test_id("ctrl-item")).to_have_count(before + 1, timeout=20000)
        check("HUB-4 create controller adds an item", add_ctrl)

        def bind_node():
            page.get_by_test_id("ctrl-item").last.click()
            expect(page.get_by_test_id("ctrl-detail")).to_be_visible()
            expect(page.get_by_test_id("ctrl-status")).not_to_be_attached()
            expect(page.get_by_test_id("ctrl-tab-node")).to_be_visible()
            expect(page.get_by_test_id("ctrl-tab-inference")).to_be_visible()
            rows = page.get_by_test_id("bind-rows")
            # click the first available "bind" button, if any
            btn = rows.locator("button", has_text="bind").first
            expect(btn).to_be_visible(timeout=20000)
            btn.click()
            expect(rows.locator("button", has_text="unbind").first).to_be_visible(timeout=20000)
        check("HUB-5 bind a node to the controller", bind_node)

        def remote_unit_form():
            page.get_by_test_id("ctrl-item").last.click()
            page.get_by_test_id("ctrl-tab-node").click()
            expect(page.get_by_test_id("remote-unit-section")).to_be_visible()
            expect(page.get_by_test_id("remote-unit-url")).to_be_visible()
            expect(page.get_by_test_id("remote-unit-create")).to_be_visible()
        check("HUB-6 remote unit form is available", remote_unit_form)

        def firewall_guide():
            page.get_by_test_id("firewall-menu").click()
            expect(page.get_by_test_id("firewall-page")).to_be_visible()
            expect(page.get_by_test_id("firewall-page")).to_contain_text("Windows firewall")
            expect(page.get_by_test_id("firewall-page")).to_contain_text("macOS firewall")
            expect(page.get_by_test_id("firewall-page")).to_contain_text("Linux firewall")
        check("HUB-7 firewall guide is available", firewall_guide)

        def serve_chat():
            page.get_by_test_id("ctrl-item").last.click()
            page.get_by_test_id("ctrl-tab-inference").click()
            d = page.get_by_test_id("ctrl-detail")
            # first model is auto-selected; select_option(index=0) can blank it (Playwright quirk)
            page.wait_for_function("()=>{const s=document.querySelector('[data-testid=serve-model]');return s&&s.value}")
            d.get_by_test_id("serve-parallel").fill("1")
            d.get_by_test_id("serve-btn").click()
            expect(page.get_by_test_id("plan-verdict")).to_contain_text(re.compile(r"RUNNING|LOADING"), timeout=180000)
            d.get_by_test_id("chat-input").fill("Reply with one word: ok")
            d.get_by_test_id("chat-send").click()
            out = d.get_by_test_id("chat-output")
            page.wait_for_function("()=>{const t=document.querySelector('[data-testid=chat-output]').innerText.trim();return t.length>1&&t!=='…'}", timeout=60000)
            assert "chat error" not in out.inner_text().lower()
        check("HUB-8 serve a model + chat returns text", serve_chat)

        page.screenshot(path="/t/hub.png", full_page=True)
    ok = sum(results)
    print(f"\n{ok}/{len(results)} hub scenarios passed")
    sys.exit(0 if ok == len(results) else 1)


if __name__ == "__main__":
    main()
