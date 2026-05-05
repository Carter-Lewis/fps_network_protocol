class WebTransportBridge {
  constructor() {
    this.transport = null;
    this.writer = null;
    this.connected = false;
    this.error = null;
    this.queue = [];
    this.streamQueue = [];
  }

  async connect(url, certHashB64) {
    try {
      const opts = {};
      if (certHashB64) {
        const raw = Uint8Array.from(atob(certHashB64), c => c.charCodeAt(0));
        opts.serverCertificateHashes = [{ algorithm: "sha-256", value: raw }];
      }
      this.transport = new WebTransport(url, opts);
      await this.transport.ready;
      this.writer = this.transport.datagrams.writable.getWriter();
      this.connected = true;
      this._readLoop();
      this._streamLoop();
      console.log("[WT] Connected, loops started");
    } catch (e) {
      this.error = String(e);
      this.connected = false;
      console.error("[WT] connect error:", e);
    }
  }
  connectAsync(url, certHashB64) { this.connect(url, certHashB64); }

  isConnectedStatus() { return this.connected; }
  getConnectionError() { return this.error; }

  sendDatagram(arr) {
    if (!this.writer) return false;
    this.writer.write(new Uint8Array(arr)).catch(e => console.error("[WT] sendDatagram error:", e));
    return true;
  }

  // Returns a comma-separated string of byte values, or null if nothing queued.
  // JavaScriptBridge.eval() in Godot 4 only supports primitive return types;
  // returning an Array (object) would silently become null.
  receiveDatagram() {
    if (this.queue.length === 0) return null;
    return Array.from(this.queue.shift()).join(',');
  }

  receiveStream() {
    if (this.streamQueue.length === 0) return null;
    const csv = Array.from(this.streamQueue.shift()).join(',');
    console.log("[WT] receiveStream:", csv);
    return csv;
  }

  async _readLoop() {
    const reader = this.transport.datagrams.readable.getReader();
    while (this.connected) {
      try {
        const { value, done } = await reader.read();
        if (done) break;
        this.queue.push(value);
      } catch (e) { console.error("[WT] _readLoop error:", e); break; }
    }
  }

  async _streamLoop() {
    console.log("[WT] _streamLoop started");
    const reader = this.transport.incomingUnidirectionalStreams.getReader();
    while (this.connected) {
      try {
        const { value: stream, done } = await reader.read();
        if (done) break;
        const r = stream.getReader();
        const chunks = [];
        while (true) {
          const { value, done } = await r.read();
          if (done) break;
          chunks.push(value);
        }
        const total = chunks.reduce((n, c) => n + c.length, 0);
        const merged = new Uint8Array(total);
        let o = 0; for (const c of chunks) { merged.set(c, o); o += c.length; }
        console.log("[WT] incoming stream bytes:", Array.from(merged));
        this.streamQueue.push(merged);
      } catch (e) { console.error("[WT] _streamLoop error:", e); break; }
    }
  }

  async sendStream(arr) {
    if (!this.transport) return false;
    try {
      const w = await this.transport.createUnidirectionalStream();
      const writer = w.getWriter();
      await writer.write(new Uint8Array(arr));
      await writer.close();
      return true;
    } catch (e) { console.error("[WT] sendStream error:", e); return false; }
  }
}

window.webtransportBridge = new WebTransportBridge();
