(async () => {
  const $ = (id) => document.getElementById(id);
  const backend = new URL(
    document.querySelector('meta[name="preview-backend"]').content || location.origin,
  );
  let snapshot;
  const term = new Terminal({
    fontSize: 13,
    fontFamily: 'Menlo, Consolas, monospace',
    cursorBlink: false,
    scrollback: 0,
    convertEol: false,
    screenReaderMode: true,
    allowProposedApi: true,
    theme: { background: '#10141b', foreground: '#dde5ee', cursor: '#ffb279' },
  });
  const fit = new FitAddon.FitAddon();
  term.loadAddon(fit);
  term.open($('terminal'));
  fit.fit();
  let socket,
    ready = false;
  const state = (text) => {
    $('status').textContent = text;
  };
  const send = (event) => {
    if (socket?.readyState === WebSocket.OPEN) socket.send(JSON.stringify(event));
  };
  term.onData((data) => {
    if (ready) send({ type: 'input', data });
  });
  term.onResize(({ cols, rows }) => send({ type: 'resize', cols, rows }));
  new ResizeObserver(() => fit.fit()).observe($('terminal'));
  term.parser.registerOscHandler(777, (data) => {
    const match = /^hn-article=(\d+)$/.exec(data);
    const item = match && snapshot?.items[match[1]];
    if (!item) return true;
    let url;
    try {
      url = new URL(item.url);
    } catch {}
    if (!url || !['http:', 'https:'].includes(url.protocol))
      url = new URL(`https://news.ycombinator.com/item?id=${item.id}`);
    $('article-host').textContent = url.hostname;
    $('original').href = url.href;
    $('article').title = item.title;
    $('article').src = url.href;
    $('article-panel').hidden = false;
    $('workspace').classList.add('reading');
    return true;
  });
  function start() {
    if (socket && socket.readyState < 2) return;
    snapshot = undefined;
    $('capture').textContent = 'Loading snapshot…';
    term.reset();
    ready = false;
    state('Starting preview…');
    $('restart').hidden = true;
    $('article-panel').hidden = true;
    $('article').removeAttribute('src');
    $('workspace').classList.remove('reading');
    socket = new WebSocket(
      `${backend.protocol === 'https:' ? 'wss' : 'ws'}://${backend.host}/session`,
    );
    const connection = socket,
      pending = [];
    let pendingBytes = 0;
    socket.binaryType = 'arraybuffer';
    let ended = false;
    socket.onopen = () => send({ type: 'resize', cols: term.cols, rows: term.rows });
    socket.onmessage = async (event) => {
      if (typeof event.data !== 'string') {
        if (ready) term.write(new Uint8Array(event.data));
        else {
          pendingBytes += event.data.byteLength;
          if (pendingBytes > 1024 * 1024) {
            connection.close();
            return;
          }
          pending.push(event.data);
        }
        return;
      }
      const value = JSON.parse(event.data);
      if (value.type === 'ready') {
        try {
          if (!/^[a-f0-9]{64}$/.test(value.hash)) throw Error('Invalid snapshot version.');
          const response = await fetch(new URL(`/snapshots/${value.hash}.json`, backend));
          if (!response.ok) throw Error('Snapshot changed. Restart the preview to reconnect.');
          const captured = await response.json();
          if (connection !== socket || connection.readyState !== WebSocket.OPEN) return;
          snapshot = captured;
          $('capture').textContent =
            `Snapshot · ${new Date(snapshot.generated_at).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' })}`;
          for (const data of pending) term.write(new Uint8Array(data));
          pending.length = 0;
          ready = true;
          state('Ready');
          if (matchMedia('(pointer:fine)').matches) term.focus();
        } catch (error) {
          ended = true;
          state(error.message);
          connection.close();
        }
      } else if (value.type === 'status') state('Starting preview…');
      else if (value.type === 'ended') {
        ready = false;
        ended = true;
        state(
          value.text === 'CLI exited'
            ? 'Preview ended. Restart to explore again.'
            : value.text === 'time limit reached'
              ? 'Your ten-minute preview ended. You can restart.'
              : value.text,
        );
      }
    };
    socket.onerror = () => {
      ended = true;
      state('Preview unavailable. Please try again shortly.');
    };
    socket.onclose = () => {
      ready = false;
      if (!ended) state('Connection closed. Restart to reconnect.');
      $('restart').hidden = false;
    };
  }
  $('restart').onclick = start;
  window.addEventListener('pagehide', () => socket?.close());
  start();
})().catch((error) => {
  document.getElementById('status').textContent = error.message;
});
