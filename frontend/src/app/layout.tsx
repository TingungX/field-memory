import type { Metadata } from 'next';
import './globals.css';

export const metadata: Metadata = {
  title: 'field-memory 控制台',
  description: 'field-memory 认知记忆场控制台',
};

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <html lang="zh-CN">
      <head>
        <meta name="viewport" content="width=device-width, initial-scale=1.0, viewport-fit=cover" />
        <meta name="theme-color" content="#f5f4f0" />
      </head>
      <body>
        {children}
      </body>
    </html>
  );
}

