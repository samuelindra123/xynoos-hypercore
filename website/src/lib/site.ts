export const repository = "https://github.com/samuelindra123/xynoos-hypercore";
export const publicSite = "https://website-xybc.vercel.app/";
export const source = (path: string) => `${repository}/blob/main/${path}`;
export const issue = (number: number) => `${repository}/issues/${number}`;
export const release = (tag: string) => `${repository}/releases/tag/${tag}`;
export const sitePath = (slug = "") => `${import.meta.env.BASE_URL.replace(/\/?$/, "/")}${slug ? slug + "/" : ""}`;

export const sections = [
  { slug: "hypercore", name: "HyperCore" },
  { slug: "downloads", name: "Downloads" },
  { slug: "documentation", name: "Documentation" },
  { slug: "developers", name: "Developers" },
  { slug: "research", name: "Research" },
  { slug: "changelog", name: "Changelog" },
  { slug: "roadmap", name: "Roadmap" },
  { slug: "about", name: "About" },
] as const;

export const standalone = "v0.1.0-preview.3";
export const debian = { version: "0.1.0~preview.3-1", tag: "debian/0.1.0-preview.3-1" };
