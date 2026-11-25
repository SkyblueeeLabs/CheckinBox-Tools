import { useEffect, useState } from "react";
import bundledAnnouncement from "../../site/announcement.json";

const FEED_URL = "https://skyblueeelabs.github.io/CheckinBox-Tools/announcement.json";
const CACHE_KEY = "checkinbox.home-announcement";
const dismissalKey = (id: string) => `${CACHE_KEY}.dismissed.${id}`;

interface Announcement {
  id: string;
  text: string;
  detailsUrl?: string;
}

function parseAnnouncement(value: unknown): Announcement | null | undefined {
  if (value === null) return null;
  if (!value || typeof value !== "object" || !("id" in value) || !("text" in value)) return;
  if (typeof value.id !== "string" || typeof value.text !== "string") return;
  const id = value.id.trim();
  const text = value.text.trim();
  if (!id || !text) return;

  if ("detailsUrl" in value && typeof value.detailsUrl === "string") {
    try {
      const url = new URL(value.detailsUrl);
      if (url.protocol === "https:") return { id, text, detailsUrl: url.href };
    } catch {
      // 详情地址无效时仍可显示正文。
    }
  }
  return { id, text };
}

function initialAnnouncement(): Announcement | null {
  try {
    const cached = localStorage.getItem(CACHE_KEY);
    if (cached !== null) {
      const announcement = parseAnnouncement(JSON.parse(cached));
      if (announcement !== undefined) return announcement;
    }
  } catch {
    // 离线首次启动或缓存异常时使用随安装包提供的公告。
  }
  return parseAnnouncement(bundledAnnouncement) ?? null;
}

function wasDismissed(id: string): boolean {
  try {
    return localStorage.getItem(dismissalKey(id)) === "1";
  } catch {
    return false;
  }
}

export default function HomeAnnouncement({ onOpenDetails }: { onOpenDetails: (url: string) => void }) {
  const [announcement, setAnnouncement] = useState(initialAnnouncement);
  const [dismissedId, setDismissedId] = useState<string | null>(null);

  useEffect(() => {
    const controller = new AbortController();
    const timeout = window.setTimeout(() => controller.abort(), 5000);
    async function refresh() {
      try {
        const response = await fetch(FEED_URL, {
          credentials: "omit",
          cache: "no-cache",
          signal: controller.signal,
        });
        if (!response.ok) return;
        const next = parseAnnouncement(await response.json());
        if (next === undefined || controller.signal.aborted) return;
        setAnnouncement(next);
        localStorage.setItem(CACHE_KEY, JSON.stringify(next));
      } catch {
        // 网络或存储不可用时继续显示已有内容。
      } finally {
        window.clearTimeout(timeout);
      }
    }
    void refresh();
    return () => {
      window.clearTimeout(timeout);
      controller.abort();
    };
  }, []);

  if (!announcement || announcement.id === dismissedId || wasDismissed(announcement.id)) return null;
  const { id, text, detailsUrl } = announcement;
  return (
    <div className="home-announcement" role="region" aria-label="公告">
      <span className="home-announcement-icon" aria-hidden="true">ⓘ</span>
      <span className="home-announcement-text" title={text}>{text}</span>
      {detailsUrl && (
        <button className="link home-announcement-details" type="button" onClick={() => onOpenDetails(detailsUrl)}>
          查看详情 <span aria-hidden="true">↗</span>
        </button>
      )}
      <button
        type="button"
        className="home-announcement-close"
        aria-label="关闭这条公告"
        title="关闭这条公告"
        onClick={() => {
          setDismissedId(id);
          try {
            localStorage.setItem(dismissalKey(id), "1");
          } catch {
            // 存储不可用时仍可在当前页面关闭。
          }
        }}
      >
        ×
      </button>
    </div>
  );
}
