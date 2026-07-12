// Ponte injetada pelo LocalZIM em toda página HTML servida do arquivo ZIM.
// Fala com o app (janela pai) por postMessage: avisa carregamento/título,
// delega links externos e recebe comandos de zoom e modo escuro.
(function () {
  if (window.__localzim) return;
  window.__localzim = true;

  function post(msg) {
    try {
      parent.postMessage(msg, "*");
    } catch (e) {}
  }

  function announce() {
    post({ type: "zim:loaded", title: document.title, href: location.href });
  }

  window.addEventListener("message", function (ev) {
    var d = ev.data || {};
    if (d.type === "zim:zoom") {
      document.documentElement.style.zoom = String(d.value);
    } else if (d.type === "zim:dark") {
      var el = document.getElementById("__localzim_dark");
      if (d.on && !el) {
        el = document.createElement("style");
        el.id = "__localzim_dark";
        el.textContent =
          "html{filter:invert(1) hue-rotate(180deg);background:#111 !important}" +
          "img,video,canvas,svg,iframe,embed,object{filter:invert(1) hue-rotate(180deg)}";
        document.documentElement.appendChild(el);
      } else if (!d.on && el) {
        el.remove();
      }
    } else if (d.type === "zim:find") {
      try {
        window.find(String(d.q || ""), false, !!d.prev, true, false, false, false);
      } catch (e) {}
    }
  });

  // Atalhos de teclado funcionam mesmo com o foco dentro do artigo:
  // a ponte encaminha o comando pro app decidir.
  document.addEventListener("keydown", function (ev) {
    var k = null;
    if (ev.altKey && ev.key === "ArrowLeft") k = "back";
    else if (ev.altKey && ev.key === "ArrowRight") k = "forward";
    else if ((ev.ctrlKey || ev.metaKey) && (ev.key === "=" || ev.key === "+")) k = "zoomin";
    else if ((ev.ctrlKey || ev.metaKey) && ev.key === "-") k = "zoomout";
    else if ((ev.ctrlKey || ev.metaKey) && ev.key === "0") k = "zoomreset";
    else if ((ev.ctrlKey || ev.metaKey) && ev.key.toLowerCase() === "k") k = "search";
    else if ((ev.ctrlKey || ev.metaKey) && ev.key.toLowerCase() === "f") k = "find";
    if (k) {
      ev.preventDefault();
      post({ type: "zim:key", key: k });
    }
  });

  // Links externos abrem no navegador do sistema, nunca dentro do leitor.
  document.addEventListener(
    "click",
    function (ev) {
      var t = ev.target;
      if (!t || !t.closest) return;
      var a = t.closest("a[href]");
      if (!a) return;
      var href = a.getAttribute("href") || "";
      if (/^(https?:|mailto:|tel:)/i.test(href)) {
        ev.preventDefault();
        ev.stopPropagation();
        post({ type: "zim:external", url: a.href });
      }
    },
    true
  );

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", announce);
  } else {
    announce();
  }
})();
