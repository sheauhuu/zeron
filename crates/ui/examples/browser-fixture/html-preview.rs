//! Actual native load_html + opaque-origin script/carousel/CSP regression.
use gpui::{AsyncApp, WindowHandle};
use std::{
    path::Path,
    time::{Duration, Instant},
};
use zeron_markdown::content_reference::{HtmlCompatibility, HtmlPreview};
use zeron_ui::shell::Shell;

const SAMPLE: &str = r#"
<style>body{background:#eff7f5;color:#164f46}.card{padding:24px;border:1px solid #89b7ac;border-radius:12px}button{padding:8px 14px;border-radius:6px;border:1px solid #89b7ac;background:#fff;color:#164f46}</style>
<h1>Sentence practice</h1><p>Compare two compact layouts.</p>
<div class="viz-carousel" aria-label="Practice layouts">
<section data-variant="Single column"><article class="card"><h2>Single column</h2><p>Listen, write, and compare your answer.</p><button id="answer" onclick="this.textContent='Submitted'">Submit</button></article></section>
<section data-variant="Two columns" hidden><article class="card"><h2>Two columns</h2><p>Answer and feedback side by side.</p></article></section>
</div>
<script>
addEventListener('DOMContentLoaded', () => setTimeout(async () => {
  try {
    const assert = (condition, message) => { if (!condition) throw Error(message); };
    let isolated = false;
    try { void parent.document.body; } catch { isolated = true; }
    assert(isolated, 'child can access parent');
    isolated = false;
    try { localStorage.setItem('preview', 'bad'); } catch { isolated = true; }
    assert(isolated, 'child has origin storage');
    let blocked = false;
    try { await fetch('data:text/plain,network'); } catch { blocked = true; }
    assert(blocked, 'connect-src escaped');
    document.querySelector('#answer').click();
    assert(document.querySelector('#answer').textContent === 'Submitted', 'inline script did not run');
    const slides = [...document.querySelectorAll('section[data-variant]')];
    assert(!slides[0].hidden && slides[1].hidden, 'initial variant');
    document.querySelector('nav button:last-child').click();
    assert(slides[0].hidden && !slides[1].hidden, 'next variant');
    document.querySelector('nav button:first-child').click();
    assert(!slides[0].hidden && slides[1].hidden, 'previous variant');
    parent.postMessage('preview:pass', '*');
  } catch (error) { parent.postMessage('preview:fail:' + error.message, '*'); }
}, 100));
</script>
"#;

pub async fn exercise(
    window: WindowHandle<Shell>,
    output: &Path,
    cx: &mut AsyncApp,
) -> anyhow::Result<()> {
    let request = HtmlPreview {
        path: "/tmp/layout-preview.html".into(),
        wide: true,
        compatibility: HtmlCompatibility::CodexVisualize,
    };
    let (_, browser) = window.update(cx, |shell, window, cx| {
        shell.fixture_open_browser(None, window, cx)
    })?;
    window
        .update(cx, |_, window, cx| {
            browser.update(cx, |browser, cx| {
                browser.fixture_document(&request, SAMPLE, window, cx)
            })
        })?
        .map_err(anyhow::Error::msg)?;
    let deadline = Instant::now() + Duration::from_secs(25);
    loop {
        let (status, page) = browser.read_with(cx, |browser, _| {
            (browser.fixture_document_status(), browser.page.clone())
        });
        anyhow::ensure!(page.error.is_none(), "HTML load failed: {page:?}");
        anyhow::ensure!(
            !status
                .as_deref()
                .is_some_and(|s| s.starts_with("preview:fail:")),
            "{status:?}"
        );
        if status.as_deref() == Some("preview:pass") {
            break;
        }
        anyhow::ensure!(
            Instant::now() < deadline,
            "HTML preview timed out: {status:?} {page:?}"
        );
        super::pause(cx, 50).await;
    }
    super::capture_frame(window, output, "html-preview", cx).await?;
    if let Some(path) = std::env::var_os("ZERON_HTML_PREVIEW_FILE") {
        let content = std::fs::read_to_string(path)?;
        window
            .update(cx, |_, window, cx| {
                browser.update(cx, |browser, cx| {
                    browser.fixture_document(&request, &content, window, cx)
                })
            })?
            .map_err(anyhow::Error::msg)?;
        super::pause(cx, 1200).await;
        super::capture_frame(window, output, "html-preview-user-file", cx).await?;
    }
    std::fs::write(
        output.join("html-preview-result.json"),
        "{\"status\":\"pass\"}\n",
    )?;
    Ok(())
}
