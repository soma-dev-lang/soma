import re,glob,sys
exec(open('outils/graphe.py').read())
sys.setrecursionlimit(100000)
idx={};low={};st=[];on=set();out=[];c=[0]
def sc(v):
    idx[v]=low[v]=c[0];c[0]+=1;st.append(v);on.add(v)
    for w in g.get(v,()):
        if w not in idx: sc(w);low[v]=min(low[v],low[w])
        elif w in on: low[v]=min(low[v],idx[w])
    if low[v]==idx[v]:
        comp=[]
        while True:
            w=st.pop();on.discard(w);comp.append(w)
            if w==v:break
        if len(comp)>1 or v in g.get(v,()): out.append(comp)
for v in list(g):
    if v not in idx: sc(v)
for comp in out: print(len(comp), sorted(comp)[:12])
# longest path depth from request
from functools import lru_cache
import functools
@functools.lru_cache(None)
def d(v): return 1+max([d(w) for w in g.get(v,()) if w in g] or [0])
top=sorted(g,key=d,reverse=True)[:5]
for v in top: print(d(v),v)
def chain(v):
    p=[v]
    while True:
        nx=[w for w in g.get(v,()) if w in g]
        if not nx: return p
        v=max(nx,key=d); p.append(v)
print(' -> '.join(chain(top[0])))
v=sys.argv[1] if len(sys.argv)>1 else "CoordDossier.rouvrir"; print(v, d(v), " -> ".join(chain(v)))
