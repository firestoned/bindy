// Right-side Page Table of Contents
// Generates an in-page navigation menu from headings

(function() {
    'use strict';

    // Wait for DOM to be ready
    window.addEventListener('DOMContentLoaded', function() {
        initPageTOC();
        highlightActiveTOCItem();

        // Update active item on scroll
        let scrollTimeout;
        window.addEventListener('scroll', function() {
            if (scrollTimeout) {
                clearTimeout(scrollTimeout);
            }
            scrollTimeout = setTimeout(highlightActiveTOCItem, 100);
        });
    });

    function initPageTOC() {
        const page = document.querySelector('.page');
        if (!page) return;

        const main = document.querySelector('main');
        if (!main) return;

        // Get all headings (h2, h3, h4) from the main content
        const headings = main.querySelectorAll('h2, h3, h4');
        if (headings.length === 0) return;

        // Create wrapper for flexbox layout inside .page
        const pageWrapper = document.createElement('div');
        pageWrapper.className = 'page-wrapper';

        // Wrap existing content
        const contentWrapper = document.createElement('div');
        contentWrapper.className = 'content-wrapper';

        // Move all children of page into content-wrapper
        while (page.firstChild) {
            contentWrapper.appendChild(page.firstChild);
        }

        // Create TOC
        const tocContainer = document.createElement('aside');
        tocContainer.className = 'page-toc';
        tocContainer.setAttribute('aria-label', 'Page navigation');

        const tocTitle = document.createElement('div');
        tocTitle.className = 'page-toc-title';
        tocTitle.textContent = 'On this page';

        const tocNav = document.createElement('nav');

        // Build TOC links
        headings.forEach(function(heading) {
            const link = document.createElement('a');
            link.href = '#' + heading.id;
            link.textContent = heading.textContent.replace(/^[0-9.]+\s*/, ''); // Remove chapter numbers
            link.className = 'toc-' + heading.tagName.toLowerCase();
            link.setAttribute('data-heading-id', heading.id);

            // Smooth scroll
            link.addEventListener('click', function(e) {
                e.preventDefault();
                heading.scrollIntoView({ behavior: 'smooth', block: 'start' });
                // Update URL hash
                history.pushState(null, null, '#' + heading.id);
                // Update active state immediately
                updateActiveLink(link);
            });

            tocNav.appendChild(link);
        });

        tocContainer.appendChild(tocTitle);
        tocContainer.appendChild(tocNav);

        // Assemble the page structure inside .page
        pageWrapper.appendChild(contentWrapper);
        pageWrapper.appendChild(tocContainer);
        page.appendChild(pageWrapper);
    }

    function highlightActiveTOCItem() {
        const headings = document.querySelectorAll('main h2, main h3, main h4');
        if (headings.length === 0) return;

        const scrollPos = window.scrollY + 100; // Offset for better UX
        let activeHeading = null;

        // Find the current heading
        for (let i = headings.length - 1; i >= 0; i--) {
            if (headings[i].offsetTop <= scrollPos) {
                activeHeading = headings[i];
                break;
            }
        }

        // Update active link
        const tocLinks = document.querySelectorAll('.page-toc a');
        tocLinks.forEach(function(link) {
            if (activeHeading && link.getAttribute('data-heading-id') === activeHeading.id) {
                link.classList.add('active');
            } else {
                link.classList.remove('active');
            }
        });
    }

    function updateActiveLink(clickedLink) {
        const tocLinks = document.querySelectorAll('.page-toc a');
        tocLinks.forEach(function(link) {
            link.classList.remove('active');
        });
        clickedLink.classList.add('active');
    }
})();
