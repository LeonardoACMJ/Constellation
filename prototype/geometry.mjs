export const TAU = Math.PI * 2;
export function point(cx, cy, r, a) { return [cx + Math.cos(a) * r, cy + Math.sin(a) * r]; }
export function wedge(cx, cy, inner, outer, from, to) {
  const a = point(cx, cy, outer, from), b = point(cx, cy, outer, to);
  const c = point(cx, cy, inner, to), d = point(cx, cy, inner, from);
  const big = to - from > Math.PI ? 1 : 0;
  return `M ${a} A ${outer} ${outer} 0 ${big} 1 ${b} L ${c} A ${inner} ${inner} 0 ${big} 0 ${d} Z`;
}
export function entries(children, page = 0, capacity = 9) {
  if (children.length <= capacity) return {items:children, pages:1, page:0};
  const count = capacity - 2, pages = Math.ceil(children.length / count);
  page = ((page % pages) + pages) % pages;
  return {items:[{id:'prev',name:'Previous',nav:-1}, ...children.slice(page*count,(page+1)*count), {id:'next',name:'More',nav:1}],pages,page};
}
export function fit(bounds, width, height, anchor, enabled = true) {
  if (!enabled) return {x:anchor.x,y:anchor.y,scale:1};
  const pad = 38, scale = Math.min(1,(width-2*pad)/(bounds.maxX-bounds.minX),(height-2*pad)/(bounds.maxY-bounds.minY));
  return {scale,x:Math.max(pad-bounds.minX*scale,Math.min(width-pad-bounds.maxX*scale,anchor.x)),y:Math.max(pad-bounds.minY*scale,Math.min(height-pad-bounds.maxY*scale,anchor.y))};
}
