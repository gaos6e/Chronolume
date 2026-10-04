import { useEffect, useState } from 'react';
import { save } from '@tauri-apps/plugin-dialog';
import { Download, Image } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { exportData, isTauriRuntime, writeChartPng } from '../api';
import type { ExportFormat, ExportPrivacy, ExportScope, UsageFilters } from '../types';
import { ActionFeedback } from './ActionFeedback';

interface ExportControlsProps {
  scope?: ExportScope;
  filters: UsageFilters;
  allowPng: boolean;
  disabled?: boolean;
}

export function ExportControls({ scope, filters, allowPng, disabled = false }: ExportControlsProps) {
  const { t } = useTranslation();
  const [format, setFormat] = useState<ExportFormat>('csv');
  const [privacy, setPrivacy] = useState<ExportPrivacy>('anonymous');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>();
  const [savedPath, setSavedPath] = useState<string>();
  const unavailable = !isTauriRuntime();
  useEffect(() => {
    setError(undefined);
    setSavedPath(undefined);
  }, [scope]);

  const exportStructured = async () => {
    if (!scope || unavailable || busy || disabled) return;
    setBusy(true);
    setError(undefined);
    setSavedPath(undefined);
    try {
      const path = await save({
        defaultPath: `chronolume-${scope}-${dateStamp()}.${format}`,
        filters: [{ name: format.toUpperCase(), extensions: [format] }],
      });
      if (!path) return;
      await exportData({ format, scope, privacy, filters }, path);
      setSavedPath(path);
    } catch (failure) {
      setError(failure);
    } finally {
      setBusy(false);
    }
  };

  const exportPng = async () => {
    if (unavailable || busy || disabled) return;
    const svg = document.querySelector<SVGSVGElement>('.chart-card .recharts-surface');
    if (!svg) { setError(t('趋势图尚未就绪，请稍后重试。')); return; }
    setBusy(true);
    setError(undefined);
    setSavedPath(undefined);
    try {
      const path = await save({
        defaultPath: `chronolume-dashboard-${dateStamp()}.png`,
        filters: [{ name: 'PNG', extensions: ['png'] }],
      });
      if (!path) return;
      const bytes = await svgToPng(svg);
      await writeChartPng(path, [...bytes]);
      setSavedPath(path);
    } catch (failure) {
      setError(failure);
    } finally {
      setBusy(false);
    }
  };

  if (!scope) return null;
  return <div className="export-group">
    <div className="export-controls" role="group" aria-label={t('导出')} title={unavailable ? t('导出仅在桌面应用中可用。') : undefined}>
    <select disabled={busy || unavailable} aria-label={t('导出格式')} value={format} onChange={(event) => setFormat(event.target.value as ExportFormat)}>
      <option value="csv">CSV</option><option value="json">JSON</option>
    </select>
    <select disabled={busy || unavailable} aria-label={t('导出隐私')} value={privacy} onChange={(event) => setPrivacy(event.target.value as ExportPrivacy)}>
      <option value="anonymous">{t('匿名路径')}</option><option value="full_path">{t('完整路径')}</option>
    </select>
    <button type="button" className="quiet-button" disabled={busy || unavailable || disabled} onClick={() => void exportStructured()}><Download />{t(busy ? '正在导出…' : '导出')}</button>
    {allowPng && <button type="button" className="icon-button" aria-label={t('导出趋势图 PNG')} disabled={busy || unavailable || disabled} onClick={() => void exportPng()}><Image /></button>}
    </div>
    <ActionFeedback error={error} />
    {savedPath && <div className="export-result" role="status">{t('已导出到')} <span>{savedPath}</span></div>}
  </div>;
}

async function svgToPng(svg: SVGSVGElement): Promise<Uint8Array> {
  const rect = svg.getBoundingClientRect();
  const width = Math.max(1, Math.round(rect.width));
  const height = Math.max(1, Math.round(rect.height));
  const clone = svg.cloneNode(true) as SVGSVGElement;
  clone.setAttribute('xmlns', 'http://www.w3.org/2000/svg');
  clone.setAttribute('width', String(width));
  clone.setAttribute('height', String(height));
  clone.style.fontFamily = getComputedStyle(svg).fontFamily;
  // Resolve CSS variables before serialization; the exported SVG has no application stylesheet.
  const originals = svg.querySelectorAll('*');
  clone.querySelectorAll('*').forEach((element, index) => {
    const style = getComputedStyle(originals[index]);
    ['fill', 'stroke', 'color'].forEach((property) => {
      if (element.getAttribute(property)?.includes('var(')) element.setAttribute(property, style.getPropertyValue(property));
    });
  });
  const blob = new Blob([new XMLSerializer().serializeToString(clone)], { type: 'image/svg+xml' });
  const image = new window.Image();
  const url = URL.createObjectURL(blob);
  try {
    await new Promise<void>((resolve, reject) => {
      image.onload = () => resolve();
      image.onerror = () => reject(new Error('Chart image could not be rendered'));
      image.src = url;
    });
    const canvas = document.createElement('canvas');
    canvas.width = width * 2;
    canvas.height = height * 2;
    const context = canvas.getContext('2d');
    if (!context) throw new Error('Canvas is unavailable');
    context.scale(2, 2);
    context.fillStyle = getComputedStyle(document.documentElement).getPropertyValue('--surface-solid').trim() || '#101219';
    context.fillRect(0, 0, width, height);
    context.drawImage(image, 0, 0, width, height);
    const png = await new Promise<Blob>((resolve, reject) =>
      canvas.toBlob((value) => value ? resolve(value) : reject(new Error('PNG encoding failed')), 'image/png'),
    );
    return new Uint8Array(await png.arrayBuffer());
  } finally {
    URL.revokeObjectURL(url);
  }
}

function dateStamp(): string {
  return new Date().toISOString().slice(0, 10);
}
