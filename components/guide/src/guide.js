// Copy buttons: each copies its code block's text exactly as shown.
(() => {
  const announce = document.getElementById('announce');
  async function copy(text) {
    try {
      await navigator.clipboard.writeText(text);
      return true;
    } catch (_) {
      // No clipboard API (an insecure context): select a hidden copy instead.
      const t = document.createElement('textarea');
      t.value = text;
      t.setAttribute('readonly', '');
      t.style.position = 'fixed';
      t.style.opacity = '0';
      document.body.appendChild(t);
      t.select();
      let ok = false;
      try { ok = document.execCommand('copy'); } catch (_) {}
      t.remove();
      return ok;
    }
  }
  document.addEventListener('click', async (e) => {
    const b = e.target.closest('button.copy');
    if (!b) return;
    const ok = await copy(b.closest('.code').querySelector('code').textContent);
    b.textContent = ok ? 'Copied' : 'Select it';
    b.classList.toggle('done', ok);
    announce.textContent = ok ? 'Copied to the clipboard' : 'Could not copy; select the text instead';
    clearTimeout(b.timer);
    b.timer = setTimeout(() => { b.textContent = 'Copy'; b.classList.remove('done'); }, 1800);
  });
  // On a phone the step list is one row: keep the current step in view.
  const row = document.querySelector('.toc ol');
  const cur = row && row.querySelector('[aria-current]');
  if (row && cur && row.scrollWidth > row.clientWidth) {
    row.scrollLeft = cur.parentElement.offsetLeft - (row.clientWidth - cur.offsetWidth) / 2;
  }
})();
