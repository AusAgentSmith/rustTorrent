import { createContext } from "react";
import {
  CategoryInfo,
  DhtStats,
  LimitsConfig,
  RqbitAPI,
  SessionStats,
} from "./api-types";

export const APIContext = createContext<RqbitAPI>({
  listTorrents: () => {
    throw new Error("Function not implemented.");
  },
  getTorrentDetails: () => {
    throw new Error("Function not implemented.");
  },
  getTorrentStats: () => {
    throw new Error("Function not implemented.");
  },
  getPeerStats: () => {
    throw new Error("Function not implemented.");
  },
  uploadTorrent: () => {
    throw new Error("Function not implemented.");
  },
  updateOnlyFiles: () => {
    throw new Error("Function not implemented.");
  },
  pause: () => {
    throw new Error("Function not implemented.");
  },
  start: () => {
    throw new Error("Function not implemented.");
  },
  forget: () => {
    throw new Error("Function not implemented.");
  },
  delete: () => {
    throw new Error("Function not implemented.");
  },
  getTorrentStreamUrl: () => {
    throw new Error("Function not implemented.");
  },
  getStreamLogsUrl: function (): string | null {
    throw new Error("Function not implemented.");
  },
  getPlaylistUrl: function (index: number): string | null {
    throw new Error("Function not implemented.");
  },
  stats: function (): Promise<SessionStats> {
    throw new Error("Function not implemented.");
  },
  getTorrentHaves: function (index: number): Promise<Uint8Array> {
    throw new Error("Function not implemented.");
  },
  getLimits: function (): Promise<LimitsConfig> {
    throw new Error("Function not implemented.");
  },
  setLimits: function (limits: LimitsConfig): Promise<void> {
    throw new Error("Function not implemented.");
  },
  getDhtStats: function (): Promise<DhtStats> {
    throw new Error("Function not implemented.");
  },
  setRustLog: function (value: string): Promise<void> {
    throw new Error("Function not implemented.");
  },
  getMetadata: function (index: number): Promise<Uint8Array> {
    throw new Error("Function not implemented.");
  },
  getCategories: function (): Promise<Record<string, CategoryInfo>> {
    throw new Error("Function not implemented.");
  },
  createCategory: function (name: string, savePath?: string): Promise<void> {
    throw new Error("Function not implemented.");
  },
  deleteCategory: function (name: string): Promise<void> {
    throw new Error("Function not implemented.");
  },
  setTorrentCategory: function (
    torrentId: number,
    category: string | null,
  ): Promise<void> {
    throw new Error("Function not implemented.");
  },
});
