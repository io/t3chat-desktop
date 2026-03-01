import { openUrl } from "@tauri-apps/plugin-opener";

async function openInBrowser() {
  const statusEl = document.querySelector<HTMLElement>("#status");
  if (!statusEl) {
    return;
  }

  try {
    await openUrl("https://t3.chat");
    statusEl.textContent = "Opened https://t3.chat in your default browser.";
  } catch {
    statusEl.textContent =
      "Could not open browser. Please open https://t3.chat manually.";
  }
}

window.addEventListener("DOMContentLoaded", () => {
  const button = document.querySelector<HTMLButtonElement>("#open-browser");
  button?.addEventListener("click", () => {
    void openInBrowser();
  });
});
