import { SiteHeader } from "@/components/site-header";

import { OccurrenceMap } from "./occurrence-map";

export default function MapPage() {
  return (
    <div className="min-h-screen bg-[#f5f7f8] text-[#182126]">
      <SiteHeader />

      <main className="mx-auto w-full max-w-7xl px-5 py-8 sm:px-8">
        <div className="mb-6">
          <h1 className="text-2xl font-semibold tracking-tight">Occurrence Map</h1>
          <p className="mt-2 text-sm text-[#65747a]">
            座標を持つOccurrenceと、地名からNominatimでGeocodingしたOccurrenceを地図上に表示します。
          </p>
        </div>
        <OccurrenceMap />
      </main>
    </div>
  );
}
