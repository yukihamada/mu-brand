#!/usr/bin/env python3
import http.server, gzip, io, os, sys
DIR="/Users/yuki/workspace/mu-brand-traffic-wt/store/static/proposals"
PORT=int(sys.argv[1]) if len(sys.argv)>1 else 8920
SITE="http://localhost:%d"%PORT
class H(http.server.BaseHTTPRequestHandler):
    def log_message(self,*a): pass
    def _send(self, body, ctype, cache="public, max-age=3600"):
        ae=self.headers.get("Accept-Encoding","")
        hdr={"Content-Type":ctype,"Cache-Control":cache,
             "Strict-Transport-Security":"max-age=63072000; includeSubDomains; preload",
             "X-Content-Type-Options":"nosniff","X-Frame-Options":"DENY",
             "Referrer-Policy":"strict-origin-when-cross-origin",
             "Content-Security-Policy":"default-src 'self'; img-src 'self' data: https:; style-src 'self'; script-src 'self'; base-uri 'self'; frame-ancestors 'none'; object-src 'none'",
             "Permissions-Policy":"geolocation=(), microphone=(), camera=()"}
        if "gzip" in ae and len(body)>200:
            buf=io.BytesIO(); 
            with gzip.GzipFile(fileobj=buf,mode="wb") as g: g.write(body)
            body=buf.getvalue(); hdr["Content-Encoding"]="gzip"; hdr["Vary"]="Accept-Encoding"
        self.send_response(200)
        for k,v in hdr.items(): self.send_header(k,v)
        self.send_header("Content-Length",str(len(body))); self.end_headers()
        self.wfile.write(body)
    def do_GET(self):
        p=self.path.split("?")[0]
        if p in("/","/index.html","/roll-brand.html"): p="/roll-brand.html"
        if p=="/favicon.ico":
            return self._send(open(os.path.join(DIR,"favicon.svg"),"rb").read(),"image/svg+xml","public, max-age=31536000, immutable")
        if p=="/robots.txt":
            return self._send(("User-agent: *\nAllow: /\nSitemap: %s/sitemap.xml\n"%SITE).encode(),"text/plain")
        if p=="/sitemap.xml":
            xml='<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9"><url><loc>%s/roll-brand.html</loc></url></urlset>'%SITE
            return self._send(xml.encode(),"application/xml")
        fp=os.path.join(DIR,p.lstrip("/"))
        if not os.path.isfile(fp): self.send_response(404); self.end_headers(); return
        ct={"html":"text/html; charset=utf-8","css":"text/css","js":"application/javascript","png":"image/png","jpg":"image/jpeg","svg":"image/svg+xml"}.get(fp.rsplit(".",1)[-1],"application/octet-stream")
        cache="public, max-age=31536000, immutable" if fp.rsplit(".",1)[-1] in("png","jpg","svg","css","js") else "public, max-age=3600"
        self._send(open(fp,"rb").read(),ct,cache)
http.server.HTTPServer(("127.0.0.1",PORT),H).serve_forever()
