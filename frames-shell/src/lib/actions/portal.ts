/**
 * Move an element to <body>, out of every ancestor's overflow, transform and
 * backdrop-filter, which would otherwise clip it or trap `position: fixed`.
 * For menus and sheets opened from inside scrolling or blurred containers.
 */
export function portal(node: HTMLElement) {
  document.body.appendChild(node);
  return {
    destroy() {
      node.remove();
    }
  };
}
