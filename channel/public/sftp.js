// SFTP Browser Module
export class SFTPBrowser {
    constructor(wsEndpoint) {
        this.wsEndpoint = wsEndpoint;
        this.socket = null;
        this.selectedNode = null;
        this.currentPath = "/";
        this.sessionId = null;
        this.breadcrumb = ["/"];
        this.pendingListings = new Map(); // msgId -> { path, items, timer }
        this.activeDownloads = new Map(); // msgId -> { filename, chunks: Map, total_chunks, total_size }
        this.msgId = 1;
        this.awaitingCredentials = false;
        this.credentialsBuffer = { username: "", password: "" };
        this.currentItems = [];
        this.previousState = null; // Store previous successful state for error recovery
        this.errorMessage = null; // Store current error for recovery

        this.setupElements();
        this.setupEventListeners();
    }

    setupElements() {
        this.container = document.getElementById("sftp-container");
        this.browser = document.getElementById("sftp-browser");
        this.pathInput = document.getElementById("sftp-path");
        this.backBtn = document.getElementById("sftp-back");
        this.refreshBtn = document.getElementById("sftp-refresh");

        this.credentialsModal = document.getElementById("sftp-credentials-modal");
        this.usernameInput = document.getElementById("sftp-username");
        this.passwordInput = document.getElementById("sftp-password");
        this.credsSubmitBtn = document.getElementById("sftp-creds-submit");
        this.credsCancelBtn = document.getElementById("sftp-creds-cancel");
    }

    setupEventListeners() {
        this.backBtn.addEventListener("click", () => this.goBack());
        this.refreshBtn.addEventListener("click", () => this.refresh());

        this.credsSubmitBtn.addEventListener("click", () => this.submitCredentials());
        this.credsCancelBtn.addEventListener("click", () => this.cancelCredentials());
        this.passwordInput.addEventListener("keypress", (e) => {
            if (e.key === "Enter") this.submitCredentials();
        });
    }

    async connect(nodeId, socket) {
        this.selectedNode = nodeId;
        this.socket = socket;
        this.currentPath = "/";
        this.breadcrumb = ["/"];
        this.pathInput.value = "/";

        this.container.style.display = "flex";
        this.browser.innerHTML = '<div class="sftp-item-loading">Loading...</div>';
        this.backBtn.disabled = true;

        // Open SFTP tunnel - socket is already created and passed in
        // The socket connection is managed externally
        if (this.socket) {
            // Socket will send the OpenTunnel message
            // Just initialize the browser
            log("SFTP socket ready");
        }
    }

    handleTunnelOpened(sessionId) {
        this.sessionId = sessionId;
        console.log("SFTP tunnel opened, session ID:", sessionId, "- Requesting directory listing for /");
        // Request directory listing when tunnel is opened
        this.listDirectory(".");
    }

    handleListItems(msgId, item, path) {
        if (!this.pendingListings.has(msgId)) {
            // Start tracking this listing
            this.pendingListings.set(msgId, {
                path: path,
                items: []
            });
        }

        const listing = this.pendingListings.get(msgId);
        listing.items.push(item);

        // Update path and UI from response data
        this.currentPath = path;
        this.pathInput.value = path;
        this.backBtn.disabled = path === "/";

        // Only render if this is the current path being viewed
        if (path === this.currentPath) {
            this.currentItems = listing.items;
            this.errorMessage = null; // Clear any previous error on successful listing
            this.renderBrowser();
            // Save successful state for recovery
            this.previousState = {
                path: path,
                items: [...listing.items]
            };
        }
    }

    listDirectory(path) {
        if (!this.socket || !this.sessionId) {
            this.browser.innerHTML = '<div class="sftp-item-loading">Not connected</div>';
            return;
        }

        // Save current state before attempting to load new directory
        if (this.currentItems.length > 0) {
            this.previousState = {
                path: this.currentPath,
                items: [...this.currentItems]
            };
        }

        this.currentPath = path;
        this.currentItems = [];
        this.browser.innerHTML = '<div class="sftp-item-loading">Loading...</div>';

        const msgId = this.msgId++;
        // Initialize tracking for this listing
        this.pendingListings.set(msgId, {
            path: path,
            items: []
        });

        // The socket is the separate SFTP WebSocket connection
        this.socket.send_sftp_list_data(this.selectedNode, this.sessionId, path, msgId);
    }

    renderBrowser() {
        this.browser.innerHTML = "";

        if (!this.currentItems || this.currentItems.length === 0) {
            const empty = document.createElement("div");
            empty.className = "sftp-item-loading";
            empty.textContent = "Empty directory";
            this.browser.appendChild(empty);
            return;
        }

        // Sort: directories first, then by name
        const sorted = this.currentItems.sort((a, b) => {
            const aIsDir = a.is_dir;
            const bIsDir = b.is_dir;
            if (aIsDir !== bIsDir) return bIsDir - aIsDir;
            return a.name.localeCompare(b.name);
        });

        sorted.forEach((item) => {
            const itemEl = document.createElement("div");
            itemEl.className = "sftp-item";

            const icon = document.createElement("div");
            icon.className = "sftp-item-icon";
            icon.textContent = item.is_dir ? "📁" : "📄";
            itemEl.appendChild(icon);

            const name = document.createElement("div");
            name.className = `sftp-item-name ${item.is_dir ? "sftp-item-dir" : "sftp-item-file"}`;
            name.textContent = item.name;
            name.title = item.name;
            itemEl.appendChild(name);

            if (!item.is_dir && item.size !== undefined) {
                const size = document.createElement("div");
                size.className = "sftp-item-size";
                size.textContent = this.formatBytes(item.size);
                itemEl.appendChild(size);

                // Add download button for files
                const downloadBtn = document.createElement("button");
                downloadBtn.className = "sftp-download-btn";
                downloadBtn.textContent = "⬇";
                downloadBtn.title = "Download";
                downloadBtn.style.cssText = "margin-left: auto; padding: 4px 12px; background-color: #3b82f6; color: white; border: none; border-radius: 4px; cursor: pointer; font-size: 16px;";
                downloadBtn.addEventListener("click", (e) => {
                    e.stopPropagation();
                    this.downloadFile(item.name);
                });
                itemEl.appendChild(downloadBtn);
            }

            if (item.is_dir) {
                itemEl.style.cursor = "pointer";
                itemEl.addEventListener("click", () => {
                    const newPath = this.normalizePath(this.currentPath, item.name);
                    this.listDirectory(newPath);
                });
            }

            this.browser.appendChild(itemEl);
        });
    }

    normalizePath(currentPath, name) {
        if (currentPath === "/") {
            return `/${name}`;
        }
        return `${currentPath}/${name}`.replace(/\/+/g, "/");
    }

    goBack() {
        if (this.currentPath === "/") return;
        const parts = this.currentPath.split("/").filter(Boolean);
        parts.pop();
        const newPath = "/" + parts.join("/");
        this.listDirectory(newPath);
    }

    refresh() {
        this.listDirectory(this.currentPath);
    }

    showCredentialsModal() {
        this.awaitingCredentials = true;
        this.usernameInput.value = "";
        this.passwordInput.value = "";
        this.credentialsModal.style.display = "block";
        this.usernameInput.focus();
    }

    hideCredentialsModal() {
        this.credentialsModal.style.display = "none";
        this.awaitingCredentials = false;
    }

    submitCredentials() {
        const username = this.usernameInput.value.trim();
        const password = this.passwordInput.value;

        if (!username || !password) {
            alert("Username and password are required");
            return;
        }

        this.hideCredentialsModal();
        this.browser.innerHTML = '<div class="sftp-item-loading">Opening SFTP tunnel...</div>';

        if (this.socket && this.selectedNode) {
            // Send credentials to open SFTP tunnel
            // When TunnelOpened is received, handleTunnelOpened will be called
            // which will automatically request directory listing
            this.socket.open_sftp_tunnel(this.selectedNode, username, password);
            console.log("Opening SFTP tunnel with credentials for node:", this.selectedNode);
        }
    }

    cancelCredentials() {
        this.hideCredentialsModal();
        this.disconnect();
    }

    handleError(kind, message) {
        // Check if it's an auth error
        if (message && message.includes("authentication") || message && message.includes("Permission denied")) {
            this.showCredentialsModal();
        } else {
            this.browser.innerHTML = `<div class="sftp-item-loading" style="color: #f87171;">Error: ${message}</div>`;
        }
    }

    handleListingError(msgId, message) {
        this.errorMessage = message;
        // If there's a msg_id, check if it matches the current pending listing
        if (msgId !== null && msgId !== undefined && this.pendingListings.has(msgId)) {
            this.pendingListings.delete(msgId);
        }

        // Display error with dismiss button and recovery option
        this.browser.innerHTML = `
            <div style="display: flex; flex-direction: column; align-items: center; justify-content: center; height: 100%; padding: 20px;">
                <div style="color: #f87171; font-weight: bold; font-size: 16px; margin-bottom: 10px;">Error</div>
                <div style="color: #f87171; text-align: center; margin-bottom: 20px; word-break: break-word;">${message}</div>
                <div style="display: flex; gap: 10px;">
                    ${this.previousState ? '<button id="sftp-error-back-btn" style="padding: 8px 16px; background-color: #3b82f6; color: white; border: none; border-radius: 4px; cursor: pointer;">Go Back</button>' : ''}
                    <button id="sftp-error-close-btn" style="padding: 8px 16px; background-color: #6b7280; color: white; border: none; border-radius: 4px; cursor: pointer;">Dismiss</button>
                </div>
            </div>
        `;

        // Add event listeners for error buttons
        const closeBtn = this.browser.querySelector('#sftp-error-close-btn');
        if (closeBtn) {
            closeBtn.addEventListener('click', () => this.dismissError());
        }

        if (this.previousState) {
            const backBtn = this.browser.querySelector('#sftp-error-back-btn');
            if (backBtn) {
                backBtn.addEventListener('click', () => this.restorePreviousState());
            }
        }
    }

    dismissError() {
        this.errorMessage = null;
        if (this.previousState) {
            // Restore previous state
            this.currentPath = this.previousState.path;
            this.currentItems = this.previousState.items;
            this.pathInput.value = this.currentPath;
            this.backBtn.disabled = this.currentPath === "/";
            this.renderBrowser();
        } else {
            // No previous state, clear the browser
            this.browser.innerHTML = '<div class="sftp-item-loading">Empty directory</div>';
        }
    }

    restorePreviousState() {
        if (this.previousState) {
            this.currentPath = this.previousState.path;
            this.currentItems = this.previousState.items;
            this.pathInput.value = this.currentPath;
            this.backBtn.disabled = this.currentPath === "/";
            this.errorMessage = null;
            this.renderBrowser();
        }
    }

    disconnect() {
        this.socket = null;
        this.sessionId = null;
        this.selectedNode = null;
        this.currentItems = [];
        this.container.style.display = "none";
        this.hideCredentialsModal();
    }

    formatBytes(bytes) {
        if (!Number.isFinite(bytes)) return "";
        const units = ["B", "KiB", "MiB", "GiB"];
        let size = bytes;
        let unit = units.shift();
        while (units.length && size >= 1024) {
            size /= 1024;
            unit = units.shift();
        }
        return `${size.toFixed(1)} ${unit}`;
    }

    downloadFile(filename) {
        if (!this.socket || !this.sessionId) {
            console.error("Cannot download: not connected");
            return;
        }

        const msgId = this.msgId++;
        console.log(`Starting download for ${filename} with msgId ${msgId}`);

        // Initialize download tracking
        this.activeDownloads.set(msgId, {
            filename: filename,
            chunks: new Map(),
            total_chunks: null,
            total_size: null,
            progressElement: null
        });

        // Create progress indicator
        const progressEl = this.createDownloadProgressElement(filename, msgId);
        this.browser.insertBefore(progressEl, this.browser.firstChild);
        this.activeDownloads.get(msgId).progressElement = progressEl;

        // Send download request
        this.socket.send_sftp_download(this.selectedNode, this.sessionId, this.currentPath, filename, msgId);
    }

    createDownloadProgressElement(filename, msgId) {
        const container = document.createElement("div");
        container.id = `download-progress-${msgId}`;
        container.style.cssText = "padding: 12px; margin-bottom: 8px; background-color: #1f2937; border-radius: 6px; border-left: 4px solid #3b82f6;";

        const header = document.createElement("div");
        header.style.cssText = "display: flex; justify-content: space-between; align-items: center; margin-bottom: 8px;";

        const filename_el = document.createElement("span");
        filename_el.textContent = `Downloading: ${filename}`;
        filename_el.style.cssText = "font-weight: 500; color: #f9fafb;";

        const cancel_btn = document.createElement("button");
        cancel_btn.textContent = "✕";
        cancel_btn.title = "Cancel";
        cancel_btn.style.cssText = "padding: 2px 8px; background-color: #ef4444; color: white; border: none; border-radius: 4px; cursor: pointer;";
        cancel_btn.addEventListener("click", () => this.cancelDownload(msgId));

        header.appendChild(filename_el);
        header.appendChild(cancel_btn);

        const progress_bar_bg = document.createElement("div");
        progress_bar_bg.style.cssText = "width: 100%; height: 8px; background-color: #374151; border-radius: 4px; overflow: hidden;";

        const progress_bar = document.createElement("div");
        progress_bar.id = `download-progress-bar-${msgId}`;
        progress_bar.style.cssText = "height: 100%; background-color: #3b82f6; transition: width 0.3s ease; width: 0%;";

        progress_bar_bg.appendChild(progress_bar);

        const info = document.createElement("div");
        info.id = `download-info-${msgId}`;
        info.style.cssText = "margin-top: 4px; font-size: 12px; color: #9ca3af;";
        info.textContent = "Initializing...";

        container.appendChild(header);
        container.appendChild(progress_bar_bg);
        container.appendChild(info);

        return container;
    }

    handleFileChunk(msgId, chunk) {
        const download = this.activeDownloads.get(msgId);
        if (!download) {
            console.warn(`Received chunk for unknown download msgId: ${msgId}`);
            return;
        }

        // Update metadata if this is the first chunk
        if (download.total_chunks === null) {
            download.total_chunks = chunk.total_chunks;
            download.total_size = chunk.total_size;
            console.log(`Download ${download.filename}: ${download.total_chunks} chunks, ${this.formatBytes(download.total_size)}`);
        }

        // Convert chunk data to Uint8Array (it comes as a regular array from JSON)
        const chunkData = new Uint8Array(chunk.data);
        
        // Store chunk
        download.chunks.set(chunk.chunk_index, chunkData);

        // Update progress
        const progress = (download.chunks.size / download.total_chunks) * 100;
        const progressBar = document.getElementById(`download-progress-bar-${msgId}`);
        const infoEl = document.getElementById(`download-info-${msgId}`);

        if (progressBar) {
            progressBar.style.width = `${progress}`;
        }

        if (infoEl) {
            const receivedSize = Array.from(download.chunks.values()).reduce((sum, data) => sum + data.length, 0);
            infoEl.textContent = `${download.chunks.size} / ${download.total_chunks} chunks (${this.formatBytes(receivedSize)} / ${this.formatBytes(download.total_size)})`;
        }

        // Check if download is complete
        if (download.chunks.size === download.total_chunks) {
            console.log(`Download complete: ${download.filename}`);
            this.finalizeDownload(msgId);
        }
    }

    finalizeDownload(msgId) {
        const download = this.activeDownloads.get(msgId);
        if (!download) return;

        // Reconstruct file from chunks in order
        const sortedChunks = Array.from(download.chunks.entries())
            .sort((a, b) => a[0] - b[0])
            .map(([_, data]) => data);

        // Convert chunks to Blob
        const blob = new Blob(sortedChunks, { type: "application/octet-stream" });

        // Create download link
        const url = URL.createObjectURL(blob);
        const a = document.createElement("a");
        a.href = url;
        a.download = download.filename;
        document.body.appendChild(a);
        a.click();
        document.body.removeChild(a);
        URL.revokeObjectURL(url);

        // Remove progress indicator
        if (download.progressElement) {
            download.progressElement.remove();
        }

        // Clean up
        this.activeDownloads.delete(msgId);
        console.log(`Download finalized and cleaned up: ${download.filename}`);
    }

    cancelDownload(msgId) {
        const download = this.activeDownloads.get(msgId);
        if (!download) return;

        console.log(`Cancelling download: ${download.filename}`);

        // Remove progress indicator
        if (download.progressElement) {
            download.progressElement.remove();
        }

        // Clean up
        this.activeDownloads.delete(msgId);
    }
}
