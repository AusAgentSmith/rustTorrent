import { useContext, useState } from "react";
import { APIContext } from "../../context";
import { useTorrentStore } from "../../stores/torrentStore";
import { Modal } from "./Modal";
import { ModalBody } from "./ModalBody";
import { ModalFooter } from "./ModalFooter";
import { Button } from "../buttons/Button";
import { FormInput } from "../forms/FormInput";
import { FormCheckbox } from "../forms/FormCheckbox";
import { Form } from "../forms/Form";
import { Spinner } from "../Spinner";
import { BsCheckCircleFill, BsXCircleFill } from "react-icons/bs";

type FileStatus = "pending" | "uploading" | "success" | "error";

interface FileEntry {
  file: File;
  status: FileStatus;
  error?: string;
}

export const MultiTorrentUploadModal = ({
  files,
  onHide,
}: {
  files: File[];
  onHide: () => void;
}) => {
  const API = useContext(APIContext);
  const refreshTorrents = useTorrentStore((state) => state.refreshTorrents);

  const [outputFolder, setOutputFolder] = useState("/downloads");
  const [startTorrent, setStartTorrent] = useState(true);
  const [entries, setEntries] = useState<FileEntry[]>(
    files.map((file) => ({ file, status: "pending" })),
  );
  const [uploading, setUploading] = useState(false);
  const [done, setDone] = useState(false);

  const handleUploadAll = async () => {
    setUploading(true);
    const updated = [...entries];

    for (let i = 0; i < updated.length; i++) {
      updated[i] = { ...updated[i], status: "uploading" };
      setEntries([...updated]);

      try {
        await API.uploadTorrent(updated[i].file, {
          overwrite: true,
          output_folder: outputFolder,
          paused: !startTorrent,
        });
        updated[i] = { ...updated[i], status: "success" };
      } catch (e: any) {
        updated[i] = {
          ...updated[i],
          status: "error",
          error: e?.text || e?.message || "Upload failed",
        };
      }
      setEntries([...updated]);
    }

    setUploading(false);
    setDone(true);
    refreshTorrents();
  };

  const statusIcon = (status: FileStatus) => {
    switch (status) {
      case "uploading":
        return <Spinner className="w-4 h-4 inline-block" />;
      case "success":
        return <BsCheckCircleFill className="text-green-500 inline-block" />;
      case "error":
        return <BsXCircleFill className="text-red-500 inline-block" />;
      default:
        return <span className="text-secondary">--</span>;
    }
  };

  return (
    <Modal isOpen={true} onClose={onHide} title="Upload Multiple Torrents">
      <ModalBody>
        <Form>
          <FormInput
            label="Output folder"
            name="multi_output_folder"
            inputType="text"
            value={outputFolder}
            onChange={(e) => setOutputFolder(e.target.value)}
          />
          <FormCheckbox
            label="Start torrents after adding"
            checked={startTorrent}
            onChange={() => setStartTorrent(!startTorrent)}
            name="start_torrent"
          />
        </Form>

        <div className="mt-3 max-h-60 overflow-y-auto border border-divider rounded">
          {entries.map((entry, idx) => (
            <div
              key={idx}
              className="flex items-center gap-2 px-3 py-1.5 border-b border-divider last:border-b-0"
            >
              <span className="w-5 flex-shrink-0">
                {statusIcon(entry.status)}
              </span>
              <span className="truncate min-w-0 flex-1">{entry.file.name}</span>
              {entry.error && (
                <span className="text-red-500 text-xs truncate max-w-[200px]">
                  {entry.error}
                </span>
              )}
            </div>
          ))}
        </div>
      </ModalBody>
      <ModalFooter>
        {uploading && <Spinner />}
        <Button onClick={onHide} variant="cancel">
          {done ? "Close" : "Cancel"}
        </Button>
        {!done && (
          <Button
            onClick={handleUploadAll}
            variant="primary"
            disabled={uploading}
          >
            Upload All ({entries.length})
          </Button>
        )}
      </ModalFooter>
    </Modal>
  );
};
