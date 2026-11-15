"""Convert icons from the free Hugeicons package to standalone SVG files.

The npm package `@hugeicons/core-free-icons` (MIT) ships each icon as an ESM
array of [tag, attrs] pairs. This writes them as 24x24 SVGs for assets/icons.

usage:
  npm pack @hugeicons/core-free-icons && tar xzf hugeicons-core-free-icons-*.tgz
  python3 tools/hugeicons-to-svg.py package/dist/esm assets/icons \
      search=Search01 play=Play:fill ...

`name=Source` writes assets/icons/<name>.svg from <Source>Icon.js; a `:fill`
suffix also fills the outline with currentColor (used for play and pause).
Then add the variant to the `icons!` list in crates/ui/src/icon.rs."""
import re,sys,os
esm,out=sys.argv[1],sys.argv[2]
def kebab(k): return re.sub(r"([A-Z])",lambda m:"-"+m.group(1).lower(),k)
for spec in sys.argv[3:]:
    name,src=spec.split("=")
    fill = src.endswith(":fill"); src=src.replace(":fill","")
    js=open(os.path.join(esm,src+"Icon.js")).read()
    els=re.findall(r'\["(\w+)", \{([^}]*)\}\]',js)
    widths=set()
    parts=[]
    for tag,attrs in els:
        kv=re.findall(r'(\w+): "([^"]*)"',attrs)
        a=[]
        for k,v in kv:
            if k=="key": continue
            if k=="strokeWidth": widths.add(v)
            if fill and k=="fill": continue
            a.append(f'{kebab(k)}="{v}"')
        if fill: a.append('fill="currentColor"')
        parts.append(f'  <{tag} {" ".join(a)} />')
    svg='<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="24" height="24" fill="none">\n'+"\n".join(parts)+"\n</svg>\n"
    open(os.path.join(out,name+".svg"),"w").write(svg)
    print(f"{name:20} <- {src:24} stroke={sorted(widths)} fill={fill}")
