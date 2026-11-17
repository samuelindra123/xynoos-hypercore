from pathlib import Path
from html.parser import HTMLParser
import re,urllib.request,subprocess,json,concurrent.futures
base=Path(__file__).resolve().parent.parent
class Parser(HTMLParser):
 def __init__(self): super().__init__();self.assets=[];self.ids=[];self.links=[]
 def handle_starttag(self,tag,attrs):
  a=dict(attrs)
  if 'id' in a:self.ids.append(a['id'])
  if tag=='a': self.links.append(a.get('href',''))
  if tag in ['link','script','img']:
   v=a.get('src',a.get('href',''))
   if v and not v.startswith(('http','#')):self.assets.append(v)
token=subprocess.check_output(['gh','auth','token'],text=True,stderr=subprocess.DEVNULL).strip()
results=[]
for i,c in enumerate('abc',4101):
 d=base/f'concept-{c}';parser=Parser();parser.feed((d/'index.html').read_text());assert len(parser.ids)==len(set(parser.ids))
 for link in parser.links:
  if link.startswith('#'):assert link[1:] in parser.ids,link
 assets=set(parser.assets+re.findall(r'url\((assets/[^)]+)\)',(d/'styles.css').read_text()))
 remote=f'https://humble-space-tribble-v6rp7jg554jw3xw7w-{i}.app.github.dev/'
 def check(args):
  path,external=args
  url=(remote if external else f'http://localhost:{i}/')+path
  with urllib.request.urlopen(urllib.request.Request(url,headers={'X-GitHub-Token':token} if external else {}),timeout=30) as r:
   data=r.read();expected=(d/('index.html' if path=='' else path)).read_bytes();assert data==expected,(i,path,'content mismatch');return r.status
 targets=[(p,external) for external in (False,True) for p in ['',*sorted(assets)]]
 with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:statuses=list(pool.map(check,targets))
 result={'concept':c.upper(),'port':i,'preview_url':remote,'visibility':'private','local_and_authenticated_preview_requests':len(statuses),'all_status_200':all(s==200 for s in statuses),'all_bodies_match_files':True,'internal_anchors_valid':True,'assets':sorted(assets)}
 results.append(result);print(f'PASS {c.upper()}: {len(statuses)} local + authenticated preview requests; HTML and all referenced assets match disk.')
(base/'http-verification.json').write_text(json.dumps(results,indent=2)+'\n')
