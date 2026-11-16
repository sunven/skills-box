import { ExternalLink } from "lucide-react";
import { openExternalUrl } from "../lib/api";
import { Button } from "./ui/button";

/** Opens a marketplace's public page; one shared affordance for every source. */
export function OfficialLink({
  url,
  label = "官方页面",
}: {
  url: string;
  label?: string;
}) {
  return (
    <Button variant="ghost" size="sm" onClick={() => void openExternalUrl(url)}>
      <ExternalLink aria-hidden="true" />
      {label}
    </Button>
  );
}
