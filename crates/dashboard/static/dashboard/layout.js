// Shared dashboard layout: header, navigation, footer, auth helpers.
// Include in every dashboard page: <script src="/static/dashboard/layout.js"></script>

(function() {
    'use strict';

    const NAV_ITEMS = [
        { href: '/dashboard', label: 'Home' },
        { href: '/dashboard/users', label: 'Users & Tenants' },
        { href: '/dashboard/tokens', label: 'API Tokens' },
        { href: '/dashboard/metrics', label: 'All Metrics' },
    ];

    function contextNavItems() {
        const path = window.location.pathname;
        const items = [];
        const nodeMatch = path.match(/^\/dashboard\/node\/([^/]+)/);
        if (nodeMatch) {
            items.push({ href: '/dashboard/metrics/' + encodeURIComponent(nodeMatch[1]), label: 'Node Metrics' });
        }
        const appMatch = path.match(/^\/dashboard\/application\/([^/]+)/);
        if (appMatch) {
            items.push({ href: '/dashboard/metrics/' + encodeURIComponent(appMatch[1]) + '?scope=app', label: 'App Metrics' });
        }
        return items;
    }

    function escapeHtml(value) {
        return String(value ?? '')
            .replace(/&/g, '&amp;')
            .replace(/</g, '&lt;')
            .replace(/>/g, '&gt;')
            .replace(/"/g, '&quot;')
            .replace(/'/g, '&#39;');
    }

    function isActive(href) {
        const path = window.location.pathname;
        if (href === '/dashboard') {
            return path === '/dashboard' || path === '/dashboard/';
        }
        return path.startsWith(href);
    }

    function renderNav() {
        const all = NAV_ITEMS.concat(contextNavItems());
        return all.map(item => {
            const cls = isActive(item.href) ? 'nav-link active' : 'nav-link';
            return `<a href="${item.href}" class="${cls}">${escapeHtml(item.label)}</a>`;
        }).join('');
    }

    function renderHeader() {
        return `<header class="dashboard-header">
            <div class="header-top">
                <h1><a href="/dashboard" class="header-title-link">PlexSpaces Dashboard</a></h1>
                <div class="header-user" id="header-user"></div>
            </div>
            <nav class="header-nav">${renderNav()}</nav>
        </header>`;
    }

    function renderFooter(systemInfo) {
        if (!systemInfo) return '';
        const version = systemInfo.version || '-';
        const uptime = systemInfo.uptime_seconds ? formatUptime(systemInfo.uptime_seconds) : '-';
        return `<footer class="dashboard-footer">
            <span>PlexSpaces ${escapeHtml(version)}</span>
            <span>Uptime: ${escapeHtml(uptime)}</span>
        </footer>`;
    }

    function formatUptime(seconds) {
        const d = Math.floor(seconds / 86400);
        const h = Math.floor((seconds % 86400) / 3600);
        const m = Math.floor((seconds % 3600) / 60);
        if (d > 0) return `${d}d ${h}h ${m}m`;
        if (h > 0) return `${h}h ${m}m`;
        return `${m}m`;
    }

    function injectLayout() {
        const container = document.querySelector('.dashboard-container');
        if (!container) return;

        // Remove existing header if present (in case pages still have one)
        const existingHeader = container.querySelector('header.dashboard-header');
        if (existingHeader) existingHeader.remove();

        // Insert shared header at the top
        container.insertAdjacentHTML('afterbegin', renderHeader());

        // Load current user
        loadCurrentUser();
    }

    async function loadCurrentUser() {
        try {
            const response = await fetch('/api/v1/auth/me', { credentials: 'include' });
            if (response.status === 401) return;
            if (!response.ok) return;
            const user = await response.json();
            const el = document.getElementById('header-user');
            if (!el) return;
            const name = user.display_name || user.email || user.user_id || '';
            const label = user.is_admin ? name + ' (Admin)' : name;
            el.innerHTML = `<span title="${escapeHtml(user.email || '')}">${escapeHtml(label)}</span> <a href="/api/v1/auth/logout" class="logout-link">Logout</a>`;
        } catch (e) { /* ignore */ }
    }

    // Shared fetchJson with auth redirect (single redirect guard)
    let _redirecting = false;
    window.dashboardFetchJson = async function(url, options) {
        if (_redirecting) throw new Error('Redirecting to login...');
        const response = await fetch(url, {
            headers: { 'Accept': 'application/json' },
            credentials: 'include',
            ...options,
        });
        if (response.status === 401) {
            if (!_redirecting) {
                _redirecting = true;
                window.location.href = '/api/v1/auth/oidc/login';
            }
            throw new Error('Redirecting to login...');
        }
        if (!response.ok) {
            throw new Error(`Request failed: ${response.status}`);
        }
        return response.json();
    };

    window.dashboardEscapeHtml = escapeHtml;
    window.dashboardRenderFooter = renderFooter;

    // Auto-inject on DOMContentLoaded
    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', injectLayout);
    } else {
        injectLayout();
    }
})();
