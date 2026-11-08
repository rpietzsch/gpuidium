(() => {
  const s = window.__gpuidiumFavicon;
  if (!s || !s.out) return "";
  const o = s.out;
  s.out = "";
  return o;
})()
