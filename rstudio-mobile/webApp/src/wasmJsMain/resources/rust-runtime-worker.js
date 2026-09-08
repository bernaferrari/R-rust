import init, { WasmRSession } from './rust-runtime/r_wasm.js';

const ready = init().then(() => new WasmRSession());
// Queue requests even during initialization: a session evaluates one call at a time.
let queue = Promise.resolve();
self.onmessage = ({ data: { id, operation, code } }) => {
  queue = queue.then(async () => {
    try {
      const session = await ready;
      let value;
      switch (operation) {
        case 'eval': value = session.eval_checked(code); break;
        case 'string': value = session.eval_string(code); break;
        case 'plot': {
          const png = session.render_png(code, 800, 600);
          let binary = '';
          for (const byte of png) binary += String.fromCharCode(byte);
          value = `<svg xmlns="http://www.w3.org/2000/svg" width="800" height="600" viewBox="0 0 800 600"><image width="800" height="600" href="data:image/png;base64,${btoa(binary)}"/></svg>`;
          break;
        }
        default: throw new Error(`Unknown runtime operation: ${operation}`);
      }
      self.postMessage({ id, value });
    } catch (error) {
      self.postMessage({ id, error: String(error) });
    }
  });
};
