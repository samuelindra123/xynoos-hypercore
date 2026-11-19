const http=require('node:http'),fs=require('node:fs'),path=require('node:path');
const root=path.resolve(__dirname,'../../website/dist'),base='/';
function preview(){return http.createServer((req,res)=>{
 const pathname=decodeURIComponent(new URL(req.url,'http://localhost').pathname);
 if(!pathname.startsWith(base)){res.writeHead(404).end('Project-site prefix required');return}
 const relative=pathname.slice(base.length);
 const file=path.resolve(root,pathname.endsWith('/')?relative+'index.html':relative);
 if(!file.startsWith(root+path.sep)||!fs.existsSync(file)||!fs.statSync(file).isFile()){res.writeHead(404).end('Not found');return}
 const mime={'.html':'text/html; charset=utf-8','.css':'text/css','.js':'text/javascript','.svg':'image/svg+xml','.woff2':'font/woff2','.jpg':'image/jpeg','.xml':'application/xml','.txt':'text/plain'};
 res.writeHead(200,{'Content-Type':mime[path.extname(file)]||'application/octet-stream'});fs.createReadStream(file).pipe(res);
})}
module.exports={preview,root,base};if(require.main===module)preview().listen(4174,'0.0.0.0',()=>console.log('Studio: http://localhost:4174'+base));
