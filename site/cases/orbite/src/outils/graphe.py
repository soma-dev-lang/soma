import re,glob,sys
g={}
for f in glob.glob('sys/*/*.cell')+glob.glob('commun/*.cell'):
    cell=None;sig=None
    for l in open(f):
        m=re.match(r'\s*skeleton (\w+)',l)
        if m: cell=m.group(1)
        m=re.match(r'\s*signal (\w+)',l)
        if m: sig=f'{cell}.{m.group(1)}'; g.setdefault(sig,set()); continue
        m=re.match(r'\s*(?:.*\s)?calls\s+(.*)',l)
        if m and sig and not l.strip().startswith('//'):
            for c in re.findall(r'\b([A-Za-z_]\w*(?:\.\w+)?)',m.group(1).split('//')[0]):
                if c in ('calls','moves','reads','writes','emits','when','via'): break
                if '.' not in c: c=f'{cell}.{c}'
                g[sig].add(c)
