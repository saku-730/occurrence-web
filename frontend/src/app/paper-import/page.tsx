import Link from "next/link";

import { PaperImportClient } from "./paper-import-client";

export default function PaperImportPage() {
  return (
    <>
      <Link
        href="/papers"
        className="fixed right-5 top-20 z-20 rounded-md border border-[#c9d2d6] bg-white px-4 py-2 text-sm font-medium text-[#344249] shadow-sm hover:bg-[#eef2f3] sm:right-8"
      >
        インポート済み論文
      </Link>
      <PaperImportClient />
    </>
  );
}
