import { Download, Star } from "lucide-react";

const compactCount = new Intl.NumberFormat("zh-CN", {
  notation: "compact",
  maximumFractionDigits: 1,
});
const exactCount = new Intl.NumberFormat("zh-CN");

export function MarketplaceStats({ stars, downloads }: {
  stars: number | null;
  downloads: number | null;
}) {
  return (
    <div className="marketplace-stats">
      {[
        { label: "收藏", count: stars, Icon: Star },
        { label: "下载量", count: downloads, Icon: Download },
      ].map(({ label, count, Icon }) => {
        const description = `${label}：${count === null ? "平台未提供" : exactCount.format(count)}`;
        return (
          <span key={label} role="group" aria-label={description} title={description}>
            <Icon aria-hidden="true" />
            {count === null ? "—" : compactCount.format(count)}
          </span>
        );
      })}
    </div>
  );
}
