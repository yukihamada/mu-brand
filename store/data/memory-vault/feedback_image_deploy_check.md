---
name: image-deploy-check
description: Always audit all image references before deploying HTML pages — never deploy HTML that references images not yet on disk
type: feedback
originSessionId: 3bb72d87-9da5-497d-88f4-78c86f601032
---
**Rule: Before any `fly deploy` (or any static site deploy), run a full image audit.**

Grep every `img/` reference from all HTML files and confirm each file exists on disk. Deploy only after 0 missing.

**Why:** In the soluna-web project, HTML was updated to reference generated images (wakayama_hongu/koya/kushimoto/yuasa) while the image generator (generate_all.py) was still running. The deploy captured the HTML but not the images → broken panels on live site. User noticed and had to ask for a fix.

**How to apply:**
```bash
IMG=cabin/img
missing=0
for f in $(grep -ohE 'img/[a-zA-Z0-9_\-]+\.(jpg|webp|png)' cabin/*.html | sed 's|img/||' | sort -u); do
  [ -f "$IMG/$f" ] || { echo "MISSING: $f"; missing=$((missing+1)); }
done
echo "Missing: $missing"
# Only run fly deploy if missing == 0
```

Never add `<img src="img/foo.jpg">` to HTML unless `cabin/img/foo.jpg` already exists. If an image generator is still running, wait for it to finish before updating HTML or deploying.