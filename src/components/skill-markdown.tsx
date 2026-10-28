import type { ReactNode } from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import rehypeSanitize, { defaultSchema } from "rehype-sanitize";
import { openExternalUrl } from "../lib/api";
import { isHttpsUrl, stripFrontmatter } from "../lib/markdown";

const sanitizeSchema = {
  ...defaultSchema,
  protocols: {
    ...defaultSchema.protocols,
    href: ["https"],
    src: ["https"],
  },
};

interface SafeLinkProps {
  href?: string;
  children?: ReactNode;
  onOpen: (url: string) => void;
}

function SafeLink({ href, children, onOpen }: SafeLinkProps) {
  if (!isHttpsUrl(href)) {
    return <span className="text-muted-foreground">{children}</span>;
  }
  return (
    <a
      href={href}
      rel="noreferrer noopener"
      className="text-primary underline underline-offset-2 hover:text-primary/80"
      onClick={(event) => {
        event.preventDefault();
        onOpen(href);
      }}
    >
      {children}
    </a>
  );
}

interface SkillMarkdownProps {
  content: string;
  onOpenUrl?: (url: string) => void;
}

/**
 * Rendered SKILL.md view.
 *
 * Security boundaries: frontmatter is hidden, raw HTML is skipped, Markdown is
 * sanitized, remote images are never loaded (they render as links), and only
 * https links are openable — through the opener, not in-app navigation.
 */
export function SkillMarkdown({ content, onOpenUrl }: SkillMarkdownProps) {
  const open = onOpenUrl ?? ((url: string) => void openExternalUrl(url));
  const components: Components = {
    a: ({ href, children }) => (
      <SafeLink href={href} onOpen={open}>
        {children}
      </SafeLink>
    ),
    img: ({ src, alt }) => {
      const url = typeof src === "string" ? src : "";
      const label = alt?.trim() || "未命名图片";
      return (
        <span className="my-1 inline-flex flex-wrap items-center gap-1 rounded border border-border bg-muted/50 px-1.5 py-0.5 text-[11px] text-muted-foreground">
          <span>图片：{label}</span>
          {isHttpsUrl(url) ? (
            <SafeLink href={url} onOpen={open}>
              打开图片
            </SafeLink>
          ) : (
            <span>（远程图片未加载）</span>
          )}
        </span>
      );
    },
  };

  return (
    <div className="skill-markdown" data-testid="skill-md-rendered">
      <ReactMarkdown
        skipHtml
        urlTransform={(url) => (isHttpsUrl(url) ? url : "")}
        rehypePlugins={[[rehypeSanitize, sanitizeSchema]]}
        components={components}
      >
        {stripFrontmatter(content)}
      </ReactMarkdown>
    </div>
  );
}
