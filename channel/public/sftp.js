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
}
