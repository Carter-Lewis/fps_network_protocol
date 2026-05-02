/**
 * WebTransport Bridge for Godot
 * 
 * This bridge provides WebTransport access to Godot's HTML5 export.
 * It handles the async nature of WebTransport and exposes synchronous-style
 * methods that Godot can call.
 */

class WebTransportBridge {
    constructor() {
        this.transport = null;
        this.reader = null;
        this.writer = null;
        this.isConnected = false;
        this.incomingQueue = [];
        this.onDataCallback = null;
        this.onConnectCallback = null;
        this.onDisconnectCallback = null;
        this.onErrorCallback = null;
    }

    /**
     * Connect to the WebTransport server
     * @param {string} url - WebTransport server URL (e.g., "https://localhost:7777")
     */
    async connect(url) {
        try {
            console.log(`[WebTransport] Connecting to ${url}`);
            this.transport = new WebTransport(url);

            // Wait for the session to be established
            await this.transport.ready;
            console.log("[WebTransport] Connected");
            this.isConnected = true;

            if (this.onConnectCallback) {
                this.onConnectCallback();
            }

            // Start reading datagrams in the background
            this.startReading();
        } catch (error) {
            console.error(`[WebTransport] Connection failed: ${error}`);
            this.isConnected = false;
            if (this.onErrorCallback) {
                this.onErrorCallback(error.toString());
            }
            throw error;
        }
    }

    /**
     * Send a datagram to the server
     * @param {Uint8Array} data - The data to send
     */
    async sendDatagram(data) {
        if (!this.isConnected || !this.transport) {
            console.error("[WebTransport] Not connected");
            return false;
        }

        try {
            const writer = this.transport.datagrams.writable.getWriter();
            await writer.write(data);
            writer.releaseLock();
            return true;
        } catch (error) {
            console.error(`[WebTransport] Send failed: ${error}`);
            if (this.onErrorCallback) {
                this.onErrorCallback(error.toString());
            }
            return false;
        }
    }

    /**
     * Receive a datagram (non-blocking, returns from queue or null)
     * @returns {Uint8Array|null} The next datagram, or null if queue is empty
     */
    receiveDatagram() {
        if (this.incomingQueue.length > 0) {
            return this.incomingQueue.shift();
        }
        return null;
    }

    /**
     * Check if connected
     * @returns {boolean} True if connected
     */
    isConnectedStatus() {
        return this.isConnected;
    }

    /**
     * Close the connection
     */
    async close() {
        if (this.transport) {
            try {
                this.transport.close();
            } catch (error) {
                console.error(`[WebTransport] Close error: ${error}`);
            }
        }
        this.isConnected = false;
        if (this.onDisconnectCallback) {
            this.onDisconnectCallback();
        }
    }

    /**
     * Start the background datagram reader
     */
    async startReading() {
        if (!this.transport) return;

        try {
            const reader = this.transport.datagrams.readable.getReader();
            while (this.isConnected) {
                try {
                    const { value, done } = await reader.read();
                    if (done) {
                        console.log("[WebTransport] Datagrams closed");
                        break;
                    }
                    // Add to queue for Godot to read
                    this.incomingQueue.push(value);
                    if (this.onDataCallback) {
                        this.onDataCallback(value);
                    }
                } catch (error) {
                    console.error(`[WebTransport] Read error: ${error}`);
                    break;
                }
            }
        } catch (error) {
            console.error(`[WebTransport] Reader setup failed: ${error}`);
        } finally {
            this.isConnected = false;
            if (this.onDisconnectCallback) {
                this.onDisconnectCallback();
            }
        }
    }

    /**
     * Set callback for incoming data
     * @param {Function} callback - Called when data arrives
     */
    setOnDataCallback(callback) {
        this.onDataCallback = callback;
    }

    /**
     * Set callback for connection established
     * @param {Function} callback - Called on successful connection
     */
    setOnConnectCallback(callback) {
        this.onConnectCallback = callback;
    }

    /**
     * Set callback for disconnection
     * @param {Function} callback - Called when disconnected
     */
    setOnDisconnectCallback(callback) {
        this.onDisconnectCallback = callback;
    }

    /**
     * Set callback for errors
     * @param {Function} callback - Called on error
     */
    setOnErrorCallback(callback) {
        this.onErrorCallback = callback;
    }
}

// Global instance for Godot to access
window.webtransportBridge = new WebTransportBridge();
