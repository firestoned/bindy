// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

(() => {
    const darkThemes = ['ayu', 'navy', 'coal'];
    const lightThemes = ['light', 'rust'];

    const classList = document.getElementsByTagName('html')[0].classList;

    let lastThemeWasLight = true;
    for (const cssClass of classList) {
        if (darkThemes.includes(cssClass)) {
            lastThemeWasLight = false;
            break;
        }
    }

    const theme = lastThemeWasLight ? 'default' : 'dark';

    // Add zoom and pan functionality to Mermaid diagrams
    function addZoomPan() {
        const svgs = document.querySelectorAll('pre.mermaid svg, .mermaid svg');
        console.log(`[Mermaid Zoom] Found ${svgs.length} SVG diagrams`);

        svgs.forEach((svg, index) => {
            if (svg.dataset.zoomEnabled) {
                console.log(`[Mermaid Zoom] SVG ${index} already initialized, skipping`);
                return; // Already initialized
            }
            svg.dataset.zoomEnabled = 'true';
            console.log(`[Mermaid Zoom] Initializing zoom/pan for SVG ${index}`);

            let scale = 1;
            let panning = false;
            let pointX = 0;
            let pointY = 0;
            let start = { x: 0, y: 0 };

            // Wrap SVG content in a group for transformations
            const g = document.createElementNS('http://www.w3.org/2000/svg', 'g');
            while (svg.firstChild) {
                g.appendChild(svg.firstChild);
            }
            svg.appendChild(g);

            // Mouse wheel zoom
            svg.addEventListener('wheel', (e) => {
                e.preventDefault();
                const delta = e.deltaY > 0 ? 0.9 : 1.1;
                scale *= delta;
                scale = Math.min(Math.max(0.5, scale), 5); // Limit zoom between 0.5x and 5x

                const rect = svg.getBoundingClientRect();
                const offsetX = e.clientX - rect.left;
                const offsetY = e.clientY - rect.top;

                pointX = offsetX - (offsetX - pointX) * delta;
                pointY = offsetY - (offsetY - pointY) * delta;

                g.style.transform = `translate(${pointX}px, ${pointY}px) scale(${scale})`;
                g.style.transformOrigin = '0 0';
            });

            // Pan on drag
            svg.addEventListener('mousedown', (e) => {
                panning = true;
                start = { x: e.clientX - pointX, y: e.clientY - pointY };
                svg.style.cursor = 'grabbing';
            });

            svg.addEventListener('mousemove', (e) => {
                if (!panning) return;
                e.preventDefault();
                pointX = e.clientX - start.x;
                pointY = e.clientY - start.y;
                g.style.transform = `translate(${pointX}px, ${pointY}px) scale(${scale})`;
            });

            svg.addEventListener('mouseup', () => {
                panning = false;
                svg.style.cursor = 'grab';
            });

            svg.addEventListener('mouseleave', () => {
                panning = false;
                svg.style.cursor = 'default';
            });

            // Double-click to reset
            svg.addEventListener('dblclick', () => {
                scale = 1;
                pointX = 0;
                pointY = 0;
                g.style.transform = 'translate(0, 0) scale(1)';
            });

            svg.style.cursor = 'grab';
        });
    }

    // Initialize Mermaid with callback to add zoom/pan after rendering
    mermaid.initialize({
        startOnLoad: true,
        theme,
        callback: function() {
            // Wait a bit for DOM to settle, then add zoom/pan
            setTimeout(addZoomPan, 200);
        }
    });

    // Also try to add zoom/pan after a delay (fallback)
    window.addEventListener('load', () => {
        setTimeout(addZoomPan, 500);
    });

    // Watch for new diagrams being added
    const observer = new MutationObserver(() => {
        setTimeout(addZoomPan, 100);
    });
    observer.observe(document.body, { childList: true, subtree: true });

    // Simplest way to make mermaid re-render the diagrams in the new theme is via refreshing the page

    for (const darkTheme of darkThemes) {
        const themeButton = document.getElementById(darkTheme);
        if (themeButton) {
            themeButton.addEventListener('click', () => {
                if (lastThemeWasLight) {
                    window.location.reload();
                }
            });
        }
    }

    for (const lightTheme of lightThemes) {
        const themeButton = document.getElementById(lightTheme);
        if (themeButton) {
            themeButton.addEventListener('click', () => {
                if (!lastThemeWasLight) {
                    window.location.reload();
                }
            });
        }
    }
})();
