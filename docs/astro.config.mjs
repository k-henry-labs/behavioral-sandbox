// @ts-check
import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";

// The sidebar is the book's order, and it is the only place that holds it: `slug` is checked
// against the pages on disk at build time, so a renamed page fails here rather than shipping a
// dead nav entry (which is what `SUMMARY.md` used to be for).
export default defineConfig({
  site: "https://docs.boxdesk.dev",
  integrations: [
    starlight({
      title: "Boxdesk",
      description:
        "A local-first desktop sandbox. Untrusted code runs in a virtual machine, on one person's machine.",
      social: [
        {
          icon: "github",
          label: "GitHub",
          href: "https://github.com/boxdesk/boxdesk",
        },
      ],
      editLink: {
        baseUrl: "https://github.com/boxdesk/boxdesk/edit/main/docs/",
      },
      sidebar: [
        { label: "Introduction", link: "/" },
        { label: "Running a sandbox", slug: "running" },
        { label: "Examples", slug: "examples" },
        { label: "Serving sandboxes", slug: "serving" },
        { label: "Architecture", slug: "architecture" },
        { label: "Control socket & IPC", slug: "control-ipc" },
        { label: "Building guest images", slug: "building-images" },
        { label: "Security", slug: "security" },
      ],
    }),
  ],
});
