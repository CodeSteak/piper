function main() {
    document.querySelectorAll('[data-copy-on-click="true"]').forEach((el) => {
        el.addEventListener('click', (evt) => {
            navigator.clipboard.writeText(el.innerText);

            const tooltip = document.createElement('div');
            tooltip.classList.add('tooltip');
            tooltip.innerText = 'Copied to clipboard!';
            tooltip.style.position = 'absolute';
            tooltip.style.top = `${evt.clientY}px`;
            tooltip.style.left = `${evt.clientX}px`;

            document.body.appendChild(tooltip);

            setTimeout(() => {
                tooltip.remove();
            }, 2500);
        });
    });

    if (window.location.hash.includes('debug')) {
        setInterval(reloadCss, 250);
    }
}

function reloadCss() {
    [...document.getElementsByTagName("link")].forEach((el) => {
        let newLink = document.createElement("link");
        newLink.rel = "stylesheet";
        newLink.href = el.href.split("?")[0] + "?v=" + Date.now();
        el.parentElement.appendChild(newLink);
        setTimeout(() => {
            el.parentElement.removeChild(el);
        }, 100);
    });
}

document.addEventListener('DOMContentLoaded', main, false);