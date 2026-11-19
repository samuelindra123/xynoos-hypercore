import { publicSite, sections } from "../lib/site";

export function GET() {
  const urls = [publicSite, ...sections.map(({ slug }) => new URL(slug + "/", publicSite).href)];
  return new Response(
    '<?xml version="1.0" encoding="UTF-8"?>\n' +
    '<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">' +
    urls.map((url) => `<url><loc>${url}</loc></url>`).join("") +
    "</urlset>\n",
    { headers: { "Content-Type": "application/xml" } },
  );
}
