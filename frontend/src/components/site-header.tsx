import Link from "next/link";

const navigationItems = [
  { href: "/", label: "Top" },
  { href: "/occurrences/search", label: "データ検索" },
  { href: "/occurrences/new", label: "データ登録" },
  { href: "/contact", label: "問い合わせ" },
  { href: "/mypage", label: "マイページ" },
];

export function SiteHeader() {
  return (
    <header className="overflow-x-auto border-b border-[#d8dfe2] bg-white">
      <div className="mx-auto flex h-16 min-w-max max-w-7xl items-center px-5 sm:px-8">
        <Link className="text-base font-semibold" href="/">
          Occurrence Web
        </Link>
        <nav className="ml-10" aria-label="メインナビゲーション">
          <ul className="flex items-center gap-6 text-sm">
            {navigationItems.map((item) => (
              <li key={item.href}>
                <Link className="hover:text-[#176b57]" href={item.href}>
                  {item.label}
                </Link>
              </li>
            ))}
          </ul>
        </nav>
      </div>
    </header>
  );
}
