import { memo, useMemo, type ReactElement, type ReactNode } from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import rehypeSanitize, { defaultSchema } from "rehype-sanitize";
import { openExternalUrl } from "../lib/api";
import { isHttpsUrl, safeMarkdownUrl, stripFrontmatter } from "../lib/markdown";
import {
  formatBytes,
  resolveSkillRelativePath,
} from "../lib/skill-content";
import { useSkillFileQuery } from "../hooks/use-skill-content";

const sanitizeSchema = {
  ...defaultSchema,
  protocols: {
    ...defaultSchema.protocols,
    href: ["https"],
    src: ["https"],
  },
};

export interface SkillInternalLinks {
  skillId: string;
  /** Directory of the previewed markdown file, relative to the skill root. */
  baseDir: string;
  onNavigate: (path: string) => void;
}

interface SafeLinkProps {
  href?: string;
  children?: ReactNode;
  onOpen: (url: string) => void;
  internalLinks?: SkillInternalLinks;
}

function SafeLink({ href, children, onOpen, internalLinks }: SafeLinkProps) {
  if (typeof href !== "string" || href === "") {
    return <span className="text-muted-foreground">{children}</span>;
  }
  if (isHttpsUrl(href)) {
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
  if (href.startsWith("#")) {
    return (
      <a
        href={href}
        className="text-primary underline underline-offset-2 hover:text-primary/80"
        onClick={(event) => {
          event.preventDefault();
          scrollToFragment(href);
        }}
      >
        {children}
      </a>
    );
  }
  const target = internalLinks
    ? resolveSkillRelativePath(internalLinks.baseDir, href)
    : null;
  if (!internalLinks || !target) {
    return <span className="text-muted-foreground">{children}</span>;
  }
  return (
    <a
      href={href}
      className="text-primary underline underline-offset-2 hover:text-primary/80"
      onClick={(event) => {
        event.preventDefault();
        internalLinks.onNavigate(target);
      }}
    >
      {children}
    </a>
  );
}

function scrollToFragment(href: string) {
  const raw = href.slice(1);
  if (!raw) return;
  let id = raw;
  try {
    id = decodeURIComponent(raw);
  } catch {
    return;
  }
  document.getElementById(id)?.scrollIntoView({ block: "start" });
}

function nodeText(node: ReactNode): string {
  if (typeof node === "string") return node;
  if (typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(nodeText).join("");
  if (node && typeof node === "object" && "props" in node) {
    return nodeText(
      (node as ReactElement<{ children?: ReactNode }>).props.children,
    );
  }
  return "";
}

function headingId(children: ReactNode): string {
  const text = nodeText(children).trim().toLowerCase();
  const slug = text
    .replace(/[^\p{L}\p{N}\s-]/gu, "")
    .trim()
    .replace(/\s+/g, "-")
    .slice(0, 80);
  return slug || "section";
}

function MarkdownHeading2({ children }: { children?: ReactNode }) {
  return <h2 id={headingId(children)}>{children}</h2>;
}

function MarkdownHeading3({ children }: { children?: ReactNode }) {
  return <h3 id={headingId(children)}>{children}</h3>;
}

function MarkdownHeading4({ children }: { children?: ReactNode }) {
  return <h4 id={headingId(children)}>{children}</h4>;
}

function openExternal(url: string) {
  void openExternalUrl(url);
}

function ImageNote({
  label,
  note,
  onOpen,
}: {
  label: string;
  note: string;
  onOpen?: () => void;
}) {
  return (
    <span className="my-1 inline-flex flex-wrap items-center gap-1 rounded-sm border border-border bg-raised px-1.5 py-0.5 text-[12px] text-muted-foreground">
      <span>图片：{label}</span>
      <span>（{note}）</span>
      {onOpen ? (
        <button
          type="button"
          className="text-primary underline underline-offset-2 hover:text-primary/80"
          onClick={onOpen}
        >
          打开文件
        </button>
      ) : null}
    </span>
  );
}

function InternalImage({
  src,
  alt,
  internalLinks,
}: {
  src: string;
  alt: string;
  internalLinks: SkillInternalLinks;
}) {
  const path = resolveSkillRelativePath(internalLinks.baseDir, src);
  const query = useSkillFileQuery(internalLinks.skillId, path);
  const label = alt || "未命名图片";
  if (!path) {
    return <ImageNote label={label} note="图片路径不在 skill 内" />;
  }
  if (query.isPending) {
    return <ImageNote label={label} note="正在加载图片…" />;
  }
  const data = query.data;
  if (data?.status === "ok" && data.dataUrl) {
    return (
      <span className="md-image-wrap">
        <img
          src={data.dataUrl}
          alt={label}
          className="md-image"
          data-testid="skill-markdown-image"
          draggable={false}
        />
        <button
          type="button"
          className="md-image-open"
          onClick={() => internalLinks.onNavigate(path)}
        >
          在内容中打开
        </button>
      </span>
    );
  }
  const note =
    data?.status === "too_large"
      ? `图片超过 ${formatBytes(data.limitBytes)} 上限`
      : data?.status === "unreadable"
        ? "图片读取失败"
        : "本地图片未加载";
  return (
    <ImageNote
      label={label}
      note={note}
      onOpen={() => internalLinks.onNavigate(path)}
    />
  );
}

interface SkillMarkdownProps {
  content: string;
  onOpenUrl?: (url: string) => void;
  internalLinks?: SkillInternalLinks;
}

/**
 * Read-only Markdown view.
 *
 * Security boundaries: frontmatter is hidden, raw HTML is skipped, Markdown is
 * sanitized, and remote images are never loaded. External links are https-only
 * and go through the opener. Relative links and images stay inside the skill:
 * they select a file in the content browser, whose backend guards every path.
 */
export const SkillMarkdown = memo(function SkillMarkdown({
  content,
  onOpenUrl,
  internalLinks,
}: SkillMarkdownProps) {
  const open = onOpenUrl ?? openExternal;
  // A stable component map keeps ReactMarkdown from remounting every node when
  // an unrelated parent re-render happens (e.g. typing in the search box).
  const components = useMemo<Components>(
    () => ({
      h1: MarkdownHeading2,
      h2: MarkdownHeading2,
      h3: MarkdownHeading3,
      h4: MarkdownHeading4,
      a: ({ href, children }) => (
        <SafeLink href={href} onOpen={open} internalLinks={internalLinks}>
          {children}
        </SafeLink>
      ),
      img: ({ src, alt }) => {
        const url = typeof src === "string" ? src : "";
        const label = alt?.trim() || "未命名图片";
        if (isHttpsUrl(url)) {
          return (
            <span className="my-1 inline-flex flex-wrap items-center gap-1 rounded-sm border border-border bg-raised px-1.5 py-0.5 text-[12px] text-muted-foreground">
              <span>图片：{label}</span>
              <SafeLink href={url} onOpen={open}>
                打开图片
              </SafeLink>
            </span>
          );
        }
        if (internalLinks && url && safeMarkdownUrl(url) === url) {
          return (
            <InternalImage
              src={url}
              alt={label}
              internalLinks={internalLinks}
            />
          );
        }
        return (
          <ImageNote
            label={label}
            note={
              /^https?:\/\//i.test(url)
                ? "远程图片未加载"
                : "本地图片未加载"
            }
          />
        );
      },
    }),
    [open, internalLinks],
  );

  return (
    <div className="skill-markdown" data-testid="skill-markdown-rendered">
      <ReactMarkdown
        skipHtml
        urlTransform={safeMarkdownUrl}
        rehypePlugins={[[rehypeSanitize, sanitizeSchema]]}
        components={components}
      >
        {stripFrontmatter(content)}
      </ReactMarkdown>
    </div>
  );
});
