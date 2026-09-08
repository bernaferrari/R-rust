(() => {
  let worker;
  let sequence = 0;
  const pending = new Map();
  function reset(message) {
    worker?.terminate();
    worker = undefined;
    for (const { reject } of pending.values()) reject(new Error(message));
    pending.clear();
  }
  globalThis.rportRust = {
    request(operation, code) {
      if (!worker) {
        worker = new Worker(new URL('rust-runtime-worker.js', document.baseURI), { type: 'module' });
        worker.onmessage = ({ data }) => {
          const request = pending.get(data.id);
          if (!request) return;
          if (data.fatal) { reset(`${data.error}. Session reset; in-memory R objects were cleared.`); return; }
          pending.delete(data.id);
          if (data.error) request.reject(new Error(data.error));
          else request.resolve(data.value);
        };
        worker.onerror = () => reset('Rust runtime worker failed. Session reset; run the script again.');
      }
      return new Promise((resolve, reject) => {
        const id = ++sequence;
        pending.set(id, { resolve, reject });
        worker.postMessage({ id, operation, code });
      });
    },
    cancel() {
      reset('Stopped. Rust session reset; in-memory R objects were cleared. Saved scripts are unchanged.');
    },
  };
})();
