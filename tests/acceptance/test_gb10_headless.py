#!/usr/bin/env python3
"""Headless browser acceptance for an already configured GB10 controller."""

import os

from playwright.sync_api import expect, sync_playwright


URL = os.environ.get("LINKCPP_HUB_URL", "http://host.docker.internal:19000")
CONTROLLER_NAME = os.environ.get("LINKCPP_CONTROLLER_NAME", "GB10 dual-slot Qwen 7B")


def main():
    with sync_playwright() as p:
        browser = p.chromium.launch()
        page = browser.new_page()
        page.set_default_timeout(30_000)
        page.goto(URL)

        controller = page.get_by_test_id("ctrl-item").filter(has_text=CONTROLLER_NAME)
        expect(controller).to_be_visible()
        controller.click()
        expect(page.locator("#cd-title")).to_contain_text(CONTROLLER_NAME)
        page.locator('[data-testid="ctrl-tab-inference"]:visible').click()
        expect(page.locator("#ctrl-inference-tab")).to_be_visible()
        detail = page.locator('[data-testid="ctrl-detail"]')
        expect(page.locator('#ctrl-inference-tab [data-testid="stop-btn"]')).to_be_visible()

        page.locator('#ctrl-inference-tab [data-testid="chat-input"]').fill(
            "Reply with exactly: headless controller inference passed"
        )
        page.locator('#ctrl-inference-tab [data-testid="chat-send"]').click()
        output = page.locator('#ctrl-inference-tab [data-testid="chat-output"]')
        expect(output).not_to_have_text("", timeout=60_000)
        expect(output).not_to_have_text("…", timeout=60_000)
        text = output.inner_text().strip()
        assert len(text) > 3 and "chat error" not in text.lower(), text
        print("HEADLESS_CONTROLLER_INFERENCE_PASS:", text)
        browser.close()


if __name__ == "__main__":
    main()
