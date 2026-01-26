// Copyright (c) 2025 Erick Bourgeois, firestoned
// SPDX-License-Identifier: MIT

// Initialize Mermaid on page load
document.addEventListener("DOMContentLoaded", function () {
  mermaid.initialize({ startOnLoad: true });

  // Add zoom/pan after initial render
  setTimeout(addZoomPan, 300);
});

// Re-render on instant navigation (Material theme)
document$.subscribe(function () {
  mermaid.init(undefined, document.querySelectorAll(".mermaid"));

  // Add zoom/pan after re-render
  setTimeout(addZoomPan, 300);
});

// Add zoom and pan functionality to all Mermaid SVG diagrams
function addZoomPan() {
  const svgs = document.querySelectorAll('pre.mermaid svg, .mermaid svg');

  svgs.forEach((svg) => {
    // Skip if already initialized
    if (svg.dataset.zoomEnabled === 'true') {
      return;
    }
    svg.dataset.zoomEnabled = 'true';

    // Zoom/pan state
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
      scale = Math.min(Math.max(0.5, scale), 5); // Limit: 0.5x to 5x

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
