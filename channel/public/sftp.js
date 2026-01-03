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
        this.deletePoll = null; // Track ongoing delete polling
        this.activeOps = 0; // Track ongoing blocking operations

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

        // Ensure container can position overlay
        if (this.container) {
            this.container.style.position = "relative";
        }

        this.createLoaderOverlay();
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
        // Request directory listing when tunnel is opened; start at root for consistency
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

                // Create button container for download and delete
                const buttonContainer = document.createElement("div");
                buttonContainer.style.cssText = "margin-left: auto; display: flex; gap: 6px;";

                // Add download button for files
                const downloadBtn = document.createElement("button");
                downloadBtn.className = "sftp-download-btn";
                downloadBtn.textContent = "⬇";
                downloadBtn.title = "Download";
                downloadBtn.style.cssText = "padding: 4px 12px; background-color: #3b82f6; color: white; border: none; border-radius: 4px; cursor: pointer; font-size: 16px;";
                downloadBtn.addEventListener("click", (e) => {
                    e.stopPropagation();
                    this.downloadFile(item.name);
                });
                buttonContainer.appendChild(downloadBtn);

                // Add delete button for files
                const deleteBtn = document.createElement("button");
                deleteBtn.className = "sftp-delete-btn";
                deleteBtn.textContent = "🗑";
                deleteBtn.title = "Delete";
                deleteBtn.style.cssText = "padding: 4px 12px; background-color: #ef4444; color: white; border: 1px solid #ef4444; border-radius: 4px; cursor: pointer; font-size: 16px;";
                deleteBtn.addEventListener("click", (e) => {
                    e.stopPropagation();
                    this.deleteFile(item.name);
                });
                buttonContainer.appendChild(deleteBtn);

                itemEl.appendChild(buttonContainer);
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
        this.hideLoader(true);
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

    formatDuration(seconds) {
        if (!Number.isFinite(seconds) || seconds < 0) return "--";
        if (seconds < 60) return `${seconds.toFixed(0)}s`;
        const m = Math.floor(seconds / 60);
        const s = Math.floor(seconds % 60);
        if (m < 60) return `${m}m ${s}s`;
        const h = Math.floor(m / 60);
        const rm = m % 60;
        return `${h}h ${rm}m`;
    }

    downloadFile(filename) {
        if (!this.socket || !this.sessionId) {
            console.error("Cannot download: not connected");
            alert("Please connect to the SFTP session first");
            return;
        }

        const msgId = this.msgId++;
        console.log(`Starting download for ${filename} with msgId ${msgId}`);

        this.showLoader("Downloading...", true);

        // Initialize download tracking
        this.activeDownloads.set(msgId, {
            filename: filename,
            chunks: new Map(),
            total_chunks: null,
            total_size: null,
            download_id: null,
            startTime: Date.now(),
            lastUpdateTime: Date.now(),
            lastReceivedBytes: 0
        });

        // Initialize pending download starts tracker if needed
        if (!this.pendingDownloadStarts) {
            this.pendingDownloadStarts = new Map();
        }

        // Create a promise to wait for download start response
        const startPromise = new Promise((resolve, reject) => {
            const timeout = setTimeout(() => {
                reject(new Error("Download start timeout"));
                this.pendingDownloadStarts.delete(msgId);
            }, 10000);

            this.pendingDownloadStarts.set(msgId, { resolve, reject, timeout });
        });

        try {
            // Send download start request
            this.socket.send_sftp_download_start(this.selectedNode, this.sessionId, this.currentPath, filename, msgId);

            // Handle the response
            startPromise
                .then(({ download_id, total_size, total_chunks }) => {
                    const download = this.activeDownloads.get(msgId);
                    if (download) {
                        download.download_id = download_id;
                        download.total_size = total_size;
                        download.total_chunks = total_chunks;
                        console.log(`Download ${filename}: ${total_chunks} chunks, ${this.formatBytes(total_size)}`);

                        // Request all chunks from the daemon
                        this.requestAllDownloadChunks(msgId, download_id, total_chunks);
                    }
                })
                .catch(err => {
                    console.error(`Download start failed: ${err}`);
                    this.activeDownloads.delete(msgId);
                    this.hideLoader();
                });
        } catch (err) {
            console.error(`Error initiating download: ${err}`);
            this.activeDownloads.delete(msgId);
            this.hideLoader();
            alert(`Failed to start download: ${err.message}`);
        }
    }

    requestAllDownloadChunks(msgId, download_id, total_chunks) {
        // Verify socket is available before scheduling any requests
        if (!this.socket) {
            console.error(`Cannot request chunks: socket is null for msgId ${msgId}`);
            this.activeDownloads.delete(msgId);
            this.hideLoader();
            return;
        }

        // Capture socket reference to avoid it becoming null during setTimeout
        const socket = this.socket;
        const selectedNode = this.selectedNode;
        const sessionId = this.sessionId;

        // Request chunks sequentially or with rate limiting
        for (let i = 0; i < total_chunks; i++) {
            setTimeout(() => {
                // Use captured references instead of this.socket
                if (!socket) {
                    console.error(`Socket disconnected during download chunk request for msgId ${msgId}`);
                    this.activeDownloads.delete(msgId);
                    this.hideLoader();
                    return;
                }

                socket.send_sftp_download_chunk(
                    selectedNode,
                    sessionId,
                    download_id,
                    i,
                    msgId
                );
            }, i * 100); // 100ms delay between chunk requests
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

        // Clean up
        this.activeDownloads.delete(msgId);
        console.log(`Download finalized and cleaned up: ${download.filename}`);
        this.hideLoader();
    }

    cancelDownload(msgId) {
        const download = this.activeDownloads.get(msgId);
        if (!download) return;

        console.log(`Cancelling download: ${download.filename}`);

        // Clean up
        this.activeDownloads.delete(msgId);
        this.hideLoader();
    }

    deleteFile(filename) {
        if (!this.socket || !this.sessionId) {
            console.error("Cannot delete: not connected");
            return;
        }

        // Ask for confirmation
        if (!confirm(`Are you sure you want to delete "${filename}"?`)) {
            return;
        }

        // Stop any previous delete poll before starting a new one
        this.stopDeletePolling(false);

        const msgId = this.msgId++;
        console.log(`Deleting file: ${filename} with msgId ${msgId}`);

        // Cancel any active downloads for this file
        for (const [downloadMsgId, download] of this.activeDownloads.entries()) {
            if (download.filename === filename) {
                console.log(`Cancelling download for deleted file: ${filename}`);
                this.activeDownloads.delete(downloadMsgId);
            }
        }

        this.showLoader("Deleting...");

        // Track polling state
        this.deletePoll = {
            filename,
            msgId,
            started: Date.now(),
            intervalId: null,
        };

        const poll = () => {
            if (!this.deletePoll) return;
            const elapsed = Date.now() - this.deletePoll.started;
            if (elapsed >= 30000) {
                console.warn(`Delete poll timed out for ${filename}`);
                this.stopDeletePolling(false);
                this.listDirectory(this.currentPath);
                return;
            }

            this.listDirectory(this.currentPath);

            // After the listing updates, check if the file is gone
            setTimeout(() => {
                if (!this.deletePoll || this.deletePoll.filename !== filename) return;
                const stillExists = this.currentItems.some((item) => item.name === filename);
                if (!stillExists) {
                    this.stopDeletePolling(true);
                }
            }, 800);
        };

        // Start polling immediately and then every ~2.5s (within 2-3s window)
        poll();
        this.deletePoll.intervalId = setInterval(poll, 2500);

        // Send delete request
        this.socket.send_sftp_delete(this.selectedNode, this.sessionId, this.currentPath, filename, msgId);
    }

    stopDeletePolling(success) {
        if (this.deletePoll && this.deletePoll.intervalId) {
            clearInterval(this.deletePoll.intervalId);
        }

        this.deletePoll = null;

        if (success) {
            // Final refresh to reflect deletion
            this.listDirectory(this.currentPath);
        }

        this.hideLoader();
    }

    createLoaderOverlay() {
        if (!this.container) return;

        const overlay = document.createElement("div");
        overlay.id = "sftp-loader-overlay";
        overlay.style.cssText = "position: absolute; inset: 0; background: rgba(15,23,42,0.68); display: none; align-items: center; justify-content: center; z-index: 5; backdrop-filter: blur(2px);";

        const box = document.createElement("div");
        box.style.cssText = "display: flex; flex-direction: column; gap: 10px; padding: 14px 18px; border-radius: 10px; background: rgba(17,24,39,0.92); border: 1px solid rgba(255,255,255,0.08); color: #e5e7eb; font-weight: 600; min-width: 240px;";

        const row = document.createElement("div");
        row.style.cssText = "display: flex; align-items: center; gap: 10px;";

        const spinner = document.createElement("span");
        spinner.style.cssText = "display: inline-block; width: 18px; height: 18px; border: 2px solid #374151; border-top: 2px solid #3b82f6; border-radius: 50%; animation: spin 0.6s linear infinite;";

        const text = document.createElement("span");
        text.id = "sftp-loader-text";
        text.textContent = "Working...";

        row.appendChild(spinner);
        row.appendChild(text);
        box.appendChild(row);

        const progressWrap = document.createElement("div");
        progressWrap.id = "sftp-loader-progress";
        progressWrap.style.cssText = "display: none; flex-direction: column; gap: 8px; width: 100%;";

        const progressRow = document.createElement("div");
        progressRow.style.cssText = "display: flex; align-items: center; gap: 10px; width: 100%;";

        const progressBarBg = document.createElement("div");
        progressBarBg.style.cssText = "flex: 1; height: 10px; background: #374151; border-radius: 9999px; overflow: hidden; box-shadow: inset 0 0 0 1px #1f2937;";

        const progressBar = document.createElement("div");
        progressBar.id = "sftp-loader-progress-bar";
        progressBar.style.cssText = "height: 100%; width: 0%; background: #10b981; transition: width 0.2s ease;";
        progressBarBg.appendChild(progressBar);

        const progressPercent = document.createElement("span");
        progressPercent.id = "sftp-loader-progress-percent";
        progressPercent.style.cssText = "min-width: 48px; text-align: right; font-variant-numeric: tabular-nums; color: #e5e7eb; font-weight: 600;";
        progressPercent.textContent = "0%";

        progressRow.appendChild(progressBarBg);
        progressRow.appendChild(progressPercent);

        const progressInfo = document.createElement("div");
        progressInfo.id = "sftp-loader-progress-info";
        progressInfo.style.cssText = "font-size: 12px; color: #cbd5e1; line-height: 1.4; text-align: left; font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono', 'Courier New', monospace;";
        progressInfo.textContent = "";

        progressWrap.appendChild(progressRow);
        progressWrap.appendChild(progressInfo);
        box.appendChild(progressWrap);

        overlay.appendChild(box);
        this.container.appendChild(overlay);

        this.loaderOverlay = overlay;
        this.loaderText = text;
        this.loaderProgressWrap = progressWrap;
        this.loaderProgressBar = progressBar;
        this.loaderProgressPercent = progressPercent;
        this.loaderProgressInfo = progressInfo;

        if (!document.getElementById("sftp-loader-spinner-style")) {
            const style = document.createElement("style");
            style.id = "sftp-loader-spinner-style";
            style.textContent = "@keyframes spin { to { transform: rotate(360deg); } }";
            document.head.appendChild(style);
        }
    }

    showLoader(message = "Working...", withProgress = false) {
        if (!this.loaderOverlay || !this.loaderText) return;
        this.activeOps = Math.max(0, this.activeOps) + 1;
        this.loaderText.textContent = message;
        if (this.loaderProgressWrap) {
            this.loaderProgressWrap.style.display = withProgress ? "flex" : "none";
            this.resetLoaderProgress();
        }
        this.loaderOverlay.style.display = "flex";
    }

    setLoaderProgress(percent, infoText = "") {
        if (!this.loaderProgressWrap || !this.loaderProgressBar || !this.loaderProgressInfo || !this.loaderProgressPercent) return;
        this.loaderProgressWrap.style.display = "flex";
        const clamped = Math.max(0, Math.min(100, percent ?? 0));
        this.loaderProgressBar.style.width = `${clamped}%`;
        this.loaderProgressPercent.textContent = `${clamped.toFixed(1)}%`;
        this.loaderProgressInfo.textContent = infoText;
    }

    resetLoaderProgress() {
        if (!this.loaderProgressWrap || !this.loaderProgressBar || !this.loaderProgressInfo || !this.loaderProgressPercent) return;
        this.loaderProgressBar.style.width = "0%";
        this.loaderProgressPercent.textContent = "0%";
        this.loaderProgressInfo.textContent = "";
    }

    hideLoader(force = false) {
        if (!this.loaderOverlay) return;
        if (!force) {
            this.activeOps = Math.max(0, this.activeOps - 1);
        } else {
            this.activeOps = 0;
        }
        if (this.activeOps <= 0) {
            this.loaderOverlay.style.display = "none";
            this.resetLoaderProgress();
        }
    }

    handleDeleteResponse(msgId, success, message) {
        // Stop any ongoing polling tied to this delete
        this.stopDeletePolling(success);

        if (success) {
            console.log(`Delete successful: ${message}`);
        } else {
            console.error(`Delete failed: ${message}`);
            this.browser.innerHTML = `<div class="sftp-item-loading" style="color: #f87171;">Delete Error: ${message}</div>`;
            // Restore previous state after showing error
            setTimeout(() => this.restorePreviousState(), 2000);
        }
    }

    initUploadListener(fileInput, uploadBtn) {
        uploadBtn.addEventListener("click", () => fileInput.click());
        fileInput.addEventListener("change", (e) => this.handleFileSelect(e));
    }

    handleFileSelect(event) {
        const files = event.target.files;
        if (!files || files.length === 0) return;

        const file = files[0];
        this.uploadFile(file);

        // Reset file input
        event.target.value = "";
    }

    async uploadFile(file) {
        if (!this.socket || !this.sessionId) {
            alert("Not connected to SFTP");
            return;
        }

        this.showLoader("Uploading...", true);

        const CHUNK_SIZE = 64 * 1024; // 64KB chunks
        const totalChunks = Math.ceil(file.size / CHUNK_SIZE);
        const msgId = this.msgId++;

        console.log(`Starting upload: ${file.name} (${this.formatBytes(file.size)}) with msgId ${msgId}`);

        // Step 1: Send upload start request and wait for upload_id
        const upload_id = await new Promise((resolve, reject) => {
            const timeout = setTimeout(() => {
                reject(new Error("Upload start timeout"));
            }, 10000);

            this.pendingUploadStarts = this.pendingUploadStarts || new Map();
            this.pendingUploadStarts.set(msgId, { resolve, reject, timeout });

            this.socket.send_sftp_upload_start(
                this.selectedNode,
                this.sessionId,
                file.name,
                this.currentPath,
                totalChunks,
                BigInt(file.size),
                msgId
            );
        });

        console.log(`Received upload_id: ${upload_id}, sending chunks...`);

        let uploadedChunks = 0;
        let uploadedBytes = 0;
        const uploadStart = performance.now();
        const RATE_BYTES_PER_SEC = 1024 * 1024; // 1 MiB/s cap

        try {
            // Step 2: Send chunks using the upload_id
            for (let i = 0; i < totalChunks; i++) {
                const start = i * CHUNK_SIZE;
                const end = Math.min(start + CHUNK_SIZE, file.size);
                const chunkData = new Uint8Array(await file.slice(start, end).arrayBuffer());

                this.socket.send_sftp_upload_chunk(
                    this.selectedNode,
                    this.sessionId,
                    upload_id,
                    i,
                    chunkData.length,
                    Array.from(chunkData),
                    null  // no msg_id needed for chunks
                );

                uploadedChunks++;
                uploadedBytes += chunkData.length;

                // Update progress in the loader overlay
                const progress = (uploadedChunks / totalChunks) * 100;
                const now = performance.now();
                const elapsedSeconds = (now - uploadStart) / 1000;
                const speed = elapsedSeconds > 0 ? uploadedBytes / elapsedSeconds : 0;
                const remainingBytes = file.size - uploadedBytes;
                const etaSeconds = speed > 0 ? remainingBytes / speed : NaN;
                const infoText = `${this.formatBytes(uploadedBytes)} / ${this.formatBytes(file.size)} • ↑ ${this.formatBytes(speed)}/s • ETA ${this.formatDuration(etaSeconds)}`;

                this.setLoaderProgress(progress, infoText);

                // Pacing: ensure average rate stays at or below 1 MiB/s
                const targetElapsedMs = (uploadedBytes / RATE_BYTES_PER_SEC) * 1000;
                const waitMs = targetElapsedMs - (performance.now() - uploadStart);
                if (waitMs > 0) {
                    await new Promise((resolve) => setTimeout(resolve, waitMs));
                }
            }
        } catch (err) {
            console.error("Upload failed:", err);
            alert(`Upload failed: ${err.message}`);
        } finally {
            this.hideLoader();
        }

        console.log(`Upload complete: ${file.name}`);

        // Refresh directory listing to show the new file
        setTimeout(() => this.listDirectory(this.currentPath), 500);
    }

    handleUploadStartResponse(msgId, upload_id) {
        if (!this.pendingUploadStarts) {
            this.pendingUploadStarts = new Map();
        }

        const pending = this.pendingUploadStarts.get(msgId);
        if (pending) {
            clearTimeout(pending.timeout);
            pending.resolve(upload_id);
            this.pendingUploadStarts.delete(msgId);
        }
    }

    handleDownloadStartResponse(msgId, download_id, total_size, total_chunks) {
        if (!this.pendingDownloadStarts) {
            this.pendingDownloadStarts = new Map();
        }

        const pending = this.pendingDownloadStarts.get(msgId);
        if (pending) {
            clearTimeout(pending.timeout);
            pending.resolve({ download_id, total_size, total_chunks });
            this.pendingDownloadStarts.delete(msgId);
        }
    }

    handleDownloadChunk(msgId, chunk) {
        const download = this.activeDownloads.get(msgId);
        if (!download) {
            console.warn(`Received download chunk for unknown download msgId: ${msgId}`);
            return;
        }

        // Safety check: ensure download metadata is set
        if (!download.total_chunks || !download.total_size) {
            console.warn(`Received chunk before download metadata was set for msgId: ${msgId}`);
            return;
        }

        // Convert chunk data to Uint8Array
        const chunkData = new Uint8Array(chunk.data);

        // Store chunk
        download.chunks.set(chunk.chunk_index, chunkData);

        // Update progress
        const progress = (download.chunks.size / download.total_chunks) * 100;
        const receivedSize = Array.from(download.chunks.values()).reduce((sum, data) => sum + data.length, 0);
        const currentTime = Date.now();
        const elapsedSeconds = (currentTime - download.startTime) / 1000;
        const speed = elapsedSeconds > 0 ? receivedSize / elapsedSeconds : 0;
        const eta = speed > 0 ? (download.total_size - receivedSize) / speed : 0;
        const infoText = `${this.formatBytes(receivedSize)} / ${this.formatBytes(download.total_size)} • ↓ ${this.formatBytes(speed)}/s • ETA: ${this.formatDuration(eta)}`;

        this.setLoaderProgress(progress, infoText);

        // Check if download complete
        if (download.chunks.size === download.total_chunks) {
            this.finalizeDownload(msgId);
        }
    }
}
