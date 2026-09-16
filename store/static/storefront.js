(function () {
  'use strict';
  document.querySelectorAll('img[data-fallback]').forEach(function (image) {
    function fallback() {
      var url = image.dataset.fallback;
      if (url && image.src !== url) {
        image.removeAttribute('data-fallback');
        image.src = url;
      }
    }
    image.addEventListener('error', fallback);
    if (image.complete && !image.naturalWidth) fallback();
  });
})();
