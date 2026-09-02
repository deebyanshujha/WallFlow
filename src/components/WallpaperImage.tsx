import { useEffect, useState } from "react";
import { wallpaperSrc } from "../lib/api";

export default function WallpaperImage({
  fileName,
  className,
  alt,
}: {
  fileName: string | null;
  className?: string;
  alt: string;
}) {
  const [src, setSrc] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    if (!fileName) {
      setSrc(null);
      return;
    }
    void wallpaperSrc(fileName).then((s) => {
      if (!cancelled) setSrc(s);
    });
    return () => {
      cancelled = true;
    };
  }, [fileName]);

  if (!src) return <div className={className} />;
  return <img className={className} src={src} alt={alt} />;
}
