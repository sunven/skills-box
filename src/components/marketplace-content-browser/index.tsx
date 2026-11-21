import { marketplaceKey, type MarketplaceDetail } from "../../lib/marketplace";
import { clawhubKey } from "../../lib/clawhub";
import { skillsShKey, type SkillsShSkill } from "../../lib/skills-sh";
import { BrowserSession } from "./browser";
import { ClawHubContent, type ClawHubContentSource } from "./clawhub";
import { SkillHubContent } from "./skillhub";
import { SkillsShContent } from "./skills-sh";

type MarketplaceContentSource =
  | { kind: "skillhub"; detail: MarketplaceDetail }
  | ClawHubContentSource
  | { kind: "skills-sh"; skill: SkillsShSkill };

export function MarketplaceContentBrowser({ source, active }: {
  source: MarketplaceContentSource; active: boolean;
}) {
  switch (source.kind) {
    case "skillhub":
      return <BrowserSession key={JSON.stringify([marketplaceKey(source.detail.coordinate), source.detail.version])} kind="skillhub">
        {(browser) => <SkillHubContent detail={source.detail} browser={browser} />}
      </BrowserSession>;
    case "clawhub":
      return <BrowserSession key={JSON.stringify([clawhubKey(source.coordinate), source.version])} kind="clawhub">
        {(browser) => <ClawHubContent source={source} active={active} browser={browser} />}
      </BrowserSession>;
    case "skills-sh":
      return <SkillsShContent key={skillsShKey(source.skill.coordinate)} skill={source.skill} active={active} />;
  }
}
