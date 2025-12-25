// PlexSpaces Dashboard JavaScript

// Initialize uPlot charts
function initActorsChart(data) {
    const container = document.getElementById('actors-chart');
    if (!container) return;

    // Parse data: { "type1": count1, "type2": count2, ... }
    const types = Object.keys(data);
    const counts = Object.values(data);

    if (types.length === 0) {
        container.innerHTML = '<p class="loading">No data available</p>';
        return;
    }

    // Prepare data for uPlot
    const chartData = [
        types,
        counts
    ];

    const opts = {
        title: "Actors by Type",
        width: container.offsetWidth,
        height: 300,
        series: [
            {},
            {
                label: "Count",
                stroke: "#667eea",
                fill: "#667eea",
                width: 2
            }
        ],
        axes: [
            {
                label: "Actor Type",
                labelSize: 12,
                labelGap: 5
            },
            {
                label: "Count",
                labelSize: 12,
                labelGap: 5
            }
        ]
    };

    // Destroy existing chart if any
    if (container._uplot) {
        container._uplot.destroy();
    }

    // Create new chart
    container._uplot = new uPlot(opts, chartData, container);
}

// Handle HTMX responses for dashboard summary
document.body.addEventListener('htmx:afterSwap', function(event) {
    if (event.detail.target.id === 'clusters-count' || 
        event.detail.target.id === 'nodes-count' ||
        event.detail.target.id === 'tenants-count' ||
        event.detail.target.id === 'applications-count') {
        // Widget value updated
        return;
    }

    // Check if this is a summary response
    if (event.detail.target.classList.contains('widget')) {
        const response = JSON.parse(event.detail.xhr.response);
        if (response.actors_by_type) {
            initActorsChart(response.actors_by_type);
        }
    }
});

// Pagination state (offset, limit per table)
window.paginationState = window.paginationState || {};

// Pagination helpers
function updatePagination(containerId, pageInfo) {
    const container = document.getElementById(containerId);
    if (!container || !pageInfo) return;

    // Use offset/limit for pagination
    const offset = pageInfo.offset || 0;
    const limit = pageInfo.limit || 50;
    const totalSize = pageInfo.total_size || 0;
    const hasNext = pageInfo.has_next || false;
    const currentPage = Math.floor(offset / limit) + 1;
    const totalPages = Math.ceil(totalSize / limit);

    // Store current state
    if (!window.paginationState[containerId]) {
        window.paginationState[containerId] = { offset: 0, limit: limit };
    }

    container.innerHTML = `
        <div style="display: flex; align-items: center; gap: 10px;">
            <button ${offset === 0 ? 'disabled' : ''} onclick="loadPage('${containerId}', ${Math.max(0, offset - limit)})">
                Previous
            </button>
            <span>Page ${currentPage} of ${totalPages || 1} (${totalSize} total)</span>
            <button ${!hasNext ? 'disabled' : ''} onclick="loadPage('${containerId}', ${offset + limit})">
                Next
            </button>
        </div>
    `;
}

function loadPage(containerId, offset) {
    // Update pagination state
    if (!window.paginationState[containerId]) {
        window.paginationState[containerId] = { offset: 0, limit: 50 };
    }
    window.paginationState[containerId].offset = offset;

    // Determine which table to reload based on container ID
    const tableMap = {
        'nodes-pagination': 'nodes-table-body',
        'applications-pagination': 'applications-table-body',
        'actors-pagination': 'actors-table-body',
        'workflows-pagination': 'workflows-table-body',
    };

    const tableBodyId = tableMap[containerId];
    if (!tableBodyId) {
        console.error('Unknown pagination container:', containerId);
        return;
    }

    const tableBody = document.getElementById(tableBodyId);
    if (!tableBody) {
        console.error('Table body not found:', tableBodyId);
        return;
    }

    // Get current HTMX URL and update with pagination params
    const hxGet = tableBody.getAttribute('hx-get');
    if (!hxGet) {
        console.error('No hx-get attribute found on table body');
        return;
    }

    // Replace :node_id placeholder if present (for node page)
    let urlString = hxGet;
    if (urlString.includes(':node_id')) {
        // Extract node_id from current URL path
        const pathParts = window.location.pathname.split('/');
        const nodeIdIndex = pathParts.indexOf('node');
        if (nodeIdIndex !== -1 && nodeIdIndex + 1 < pathParts.length) {
            const nodeId = pathParts[nodeIdIndex + 1];
            urlString = urlString.replace(/:node_id/g, nodeId);
        } else {
            console.error('Could not find node_id in URL path');
            return;
        }
    }

    // Parse URL and add/update pagination params
    const url = new URL(urlString, window.location.origin);
    url.searchParams.set('offset', offset.toString());
    url.searchParams.set('limit', window.paginationState[containerId].limit.toString());

    // Update hx-get and trigger reload
    // Preserve relative path if original was relative
    const newUrl = hxGet.startsWith('http://') || hxGet.startsWith('https://') 
        ? url.toString() 
        : url.pathname + url.search;
    tableBody.setAttribute('hx-get', newUrl);
    htmx.trigger(tableBody, 'load');
}

function loadNextPage(containerId) {
    const state = window.paginationState[containerId] || { offset: 0, limit: 50 };
    loadPage(containerId, state.offset + state.limit);
}

function loadPrevPage(containerId) {
    const state = window.paginationState[containerId] || { offset: 0, limit: 50 };
    loadPage(containerId, Math.max(0, state.offset - state.limit));
}

// Format timestamps
function formatTimestamp(timestamp) {
    if (!timestamp) return '-';
    const date = new Date(timestamp.seconds * 1000 + timestamp.nanos / 1000000);
    return date.toLocaleString();
}

// Format bytes
function formatBytes(bytes) {
    if (!bytes) return '-';
    const sizes = ['B', 'KB', 'MB', 'GB'];
    if (bytes === 0) return '0 B';
    const i = Math.floor(Math.log(bytes) / Math.log(1024));
    return Math.round(bytes / Math.pow(1024, i) * 100) / 100 + ' ' + sizes[i];
}

// Export for use in templates
window.dashboardUtils = {
    formatTimestamp,
    formatBytes,
    initActorsChart,
    updatePagination
};
