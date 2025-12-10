(function (global, factory) {
    if (typeof module === "object" && typeof module.exports === "object") {
        module.exports = factory();
    } else {
        global.FitAddon = factory();
    }
})(typeof self !== "undefined" ? self : this, function () {
    class FitAddon {
        activate(terminal) {
            this._terminal = terminal;
        }
        dispose() {}
        fit() {
            const dims = this.proposeDimensions();
            if (!this._terminal || !dims) {
                return;
            }
            const { cols, rows } = dims;
            if (this._terminal.cols !== cols || this._terminal.rows !== rows) {
                this._terminal.resize(cols, rows);
            }
        }
        proposeDimensions() {
            if (!this._terminal) {
                return undefined;
            }
            const parentElement = this._terminal.element
                ? this._terminal.element.parentElement
                : undefined;
            const core = this._terminal._core;
            if (!parentElement || !core || !core._renderService) {
                return undefined;
            }
            const style = window.getComputedStyle(parentElement);
            const height =
                parseInt(style.getPropertyValue("height"), 10) -
                (parseInt(style.getPropertyValue("padding-top"), 10) +
                    parseInt(style.getPropertyValue("padding-bottom"), 10));
            const width =
                parseInt(style.getPropertyValue("width"), 10) -
                (parseInt(style.getPropertyValue("padding-left"), 10) +
                    parseInt(style.getPropertyValue("padding-right"), 10));
            const dims = core._renderService.dimensions;
            const cols = Math.max(Math.floor(width / dims.css.cell.width), 2);
            const rows = Math.max(Math.floor(height / dims.css.cell.height), 1);
            return { cols, rows };
        }
    }

    return { FitAddon };
});
