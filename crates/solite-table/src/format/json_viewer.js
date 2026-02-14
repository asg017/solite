(function() {
    var container = document.currentScript.parentElement;
    var cells = container.querySelectorAll('.solite-json-cell');
    var PREVIEW_KEYS = 5;
    var PAGE_SIZE = 20;
    var MAX_STR = 120;
    var STORAGE_PREFIX = 'solite-json-';

    function loadExpanded(key) {
        try {
            var v = sessionStorage.getItem(STORAGE_PREFIX + key);
            return v ? JSON.parse(v) : {};
        } catch(e) { return {}; }
    }

    function saveExpanded(key, expanded) {
        try {
            sessionStorage.setItem(STORAGE_PREFIX + key, JSON.stringify(expanded));
        } catch(e) {}
    }

    function esc(s) {
        return s.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;');
    }

    function truncStr(s) {
        return s.length > MAX_STR ? s.slice(0, MAX_STR - 1) + '\u2026' : s;
    }

    function preview(val) {
        if (val === null) return '<span class="jt-null">null</span>';
        if (typeof val === 'boolean') return '<span class="jt-bool">' + val + '</span>';
        if (typeof val === 'number') return '<span class="jt-num">' + val + '</span>';
        if (typeof val === 'string') {
            var s = val.length > 40 ? val.slice(0,37) + '\u2026' : val;
            return '<span class="jt-str">"' + esc(s) + '"</span>';
        }
        if (Array.isArray(val)) return '<span class="jt-bracket">Array(' + val.length + ')</span>';
        return '<span class="jt-bracket">Object{\u2026}</span>';
    }

    function inlinePreview(val) {
        if (Array.isArray(val)) {
            var parts = [];
            for (var i = 0; i < Math.min(val.length, PREVIEW_KEYS); i++) parts.push(preview(val[i]));
            var s = parts.join(', ');
            if (val.length > PREVIEW_KEYS) s += ', \u2026';
            return '<span class="jt-bracket">[</span>' + s + '<span class="jt-bracket">]</span>';
        }
        if (val && typeof val === 'object') {
            var keys = Object.keys(val);
            var parts = [];
            for (var i = 0; i < Math.min(keys.length, PREVIEW_KEYS); i++) {
                parts.push('<span class="jt-key">' + esc(keys[i]) + '</span>: ' + preview(val[keys[i]]));
            }
            var s = parts.join(', ');
            if (keys.length > PREVIEW_KEYS) s += ', \u2026';
            return '<span class="jt-bracket">{</span>' + s + '<span class="jt-bracket">}</span>';
        }
        return preview(val);
    }

    function renderValue(val, depth, path, state) {
        if (val === null) return '<span class="jt-null">null</span>';
        if (typeof val === 'boolean') return '<span class="jt-bool">' + val + '</span>';
        if (typeof val === 'number') return '<span class="jt-num">' + val + '</span>';
        if (typeof val === 'string') return '<span class="jt-str">"' + esc(truncStr(val)) + '"</span>';
        return buildCollapsible(val, depth, path, state);
    }

    function indent(depth) {
        var s = '';
        for (var i = 0; i < depth; i++) s += '<span class="jt-indent"></span>';
        return s;
    }

    function buildCollapsible(val, depth, path, state) {
        var isArr = Array.isArray(val);
        var keys = isArr ? null : Object.keys(val);
        var len = isArr ? val.length : keys.length;
        var open = isArr ? '[' : '{';
        var close = isArr ? ']' : '}';
        var label = isArr ? 'Array(' + len + ')' : 'Object';
        var startExpanded = !!state.expanded[path];

        var el = document.createElement('span');

        // collapsed row
        var collapsedRow = document.createElement('span');
        collapsedRow.className = 'jt-row';
        collapsedRow.innerHTML =
            '<span class="jt-caret">\u25B6</span> ' +
            '<span class="jt-inline">' +
            '<span class="jt-preview">' + label + ' </span>' +
            inlinePreview(val) +
            '</span>';
        el.appendChild(collapsedRow);

        // expanded container
        var expandedEl = document.createElement('span');

        var expandedHeader = document.createElement('span');
        expandedHeader.className = 'jt-row';
        expandedHeader.innerHTML =
            '<span class="jt-caret">\u25BC</span> ' +
            '<span class="jt-preview">' + label + '</span> ' +
            '<span class="jt-bracket">' + esc(open) + '</span>';
        expandedEl.appendChild(expandedHeader);

        var entriesEl = document.createElement('span');
        var shown = 0;

        function showMore() {
            var batch = Math.min(shown + PAGE_SIZE, len);
            for (var i = shown; i < batch; i++) {
                var row = document.createElement('div');
                row.className = 'jt-row';
                var k = isArr ? i : keys[i];
                var v = isArr ? val[i] : val[k];
                var comma = i < len - 1 ? ',' : '';
                var childPath = isArr ? path + '[' + k + ']' : path + '.' + k;
                var keyHtml = isArr
                    ? '<span class="jt-num" title="' + esc(childPath) + '">' + k + '</span>'
                    : '<span class="jt-key" title="' + esc(childPath) + '">' + esc(String(k)) + '</span>';
                row.innerHTML = indent(depth + 1) + keyHtml + ': ';
                var valNode = document.createElement('span');
                var childVal = renderValue(v, depth + 1, childPath, state);
                if (typeof childVal === 'string') {
                    valNode.innerHTML = childVal + comma;
                } else {
                    valNode.appendChild(childVal);
                    if (comma) {
                        var commaSpan = document.createTextNode(comma);
                        valNode.appendChild(commaSpan);
                    }
                }
                row.appendChild(valNode);
                entriesEl.appendChild(row);
            }
            shown = batch;
            if (shown < len) {
                var more = document.createElement('div');
                more.className = 'jt-row jt-more';
                more.innerHTML = indent(depth + 1) + '\u2026 ' + (len - shown) + ' more';
                more.addEventListener('click', function(e) {
                    e.stopPropagation();
                    more.remove();
                    showMore();
                });
                entriesEl.appendChild(more);
            }
        }

        showMore();
        expandedEl.appendChild(entriesEl);

        var closingRow = document.createElement('div');
        closingRow.className = 'jt-row';
        closingRow.innerHTML = indent(depth) + '<span class="jt-bracket">' + esc(close) + '</span>';
        expandedEl.appendChild(closingRow);

        el.appendChild(expandedEl);

        // Set initial state
        var collapsed = !startExpanded;
        collapsedRow.style.display = collapsed ? '' : 'none';
        expandedEl.style.display = collapsed ? 'none' : '';

        function toggle(e) {
            e.stopPropagation();
            collapsed = !collapsed;
            collapsedRow.style.display = collapsed ? '' : 'none';
            expandedEl.style.display = collapsed ? 'none' : '';
            if (collapsed) {
                delete state.expanded[path];
            } else {
                state.expanded[path] = 1;
            }
            saveExpanded(state.key, state.expanded);
        }
        collapsedRow.style.cursor = 'pointer';
        expandedHeader.style.cursor = 'pointer';
        collapsedRow.addEventListener('click', toggle);
        expandedHeader.addEventListener('click', toggle);

        return el;
    }

    cells.forEach(function(cell, idx) {
        try {
            var raw = cell.getAttribute('data-json');
            var val = JSON.parse(raw);
            // Only build tree for objects/arrays; primitives keep flat rendering
            if (val === null || typeof val !== 'object') return;
            var key = String(idx);
            var state = { key: key, expanded: loadExpanded(key) };
            var tree = document.createElement('span');
            tree.className = 'solite-json-tree';
            var rendered = renderValue(val, 0, '$', state);
            if (typeof rendered === 'string') {
                tree.innerHTML = rendered;
            } else {
                tree.appendChild(rendered);
            }
            var copyBtn = document.createElement('span');
            copyBtn.className = 'jt-copy';
            copyBtn.textContent = 'Copy';
            copyBtn.addEventListener('click', function(e) {
                e.stopPropagation();
                navigator.clipboard.writeText(raw).then(function() {
                    copyBtn.textContent = 'Copied!';
                    setTimeout(function() { copyBtn.textContent = 'Copy'; }, 1500);
                });
            });
            cell.innerHTML = '';
            cell.appendChild(copyBtn);
            cell.appendChild(tree);
        } catch(e) { /* keep fallback */ }
    });
})();
