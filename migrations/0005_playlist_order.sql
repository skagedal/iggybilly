-- The clips carrying a label form a playlist, and a playlist has an
-- order. That order is a property of the *membership*, not of the clip:
-- a clip on three labels sits in three playlists and has an independent
-- place in each, so the column belongs on the join table and nowhere
-- else.
--
-- Positions are sparse: a gap of 1024 between neighbours. Moving one
-- clip then writes exactly one row — its new position is the midpoint
-- between the two it was dropped between — so two people dragging
-- different clips in the same playlist do not overwrite each other's
-- work, and a drag costs one UPDATE rather than a rewrite of the list.
-- When a gap is used up (the neighbours are less than two apart) the
-- label's rows are renumbered 0, 1024, 2048, … in the same transaction;
-- see queries::labels::reorder.
--
-- Positions are neither unique nor required to be contiguous: two rows
-- that end up equal through a race are broken by clip_id everywhere the
-- order is read, so the list is always totally ordered even when the
-- column is not.
--
-- Existing rows are backfilled in the order the filtered list shows
-- them today — newest upload first — so nothing appears to move on the
-- day this ships. From then on the order is whatever people drag it to.
ALTER TABLE clip_labels ADD COLUMN position INTEGER NOT NULL DEFAULT 0;

UPDATE clip_labels
SET position = 1024 * (
    SELECT COUNT(*)
    FROM clip_labels peer
    JOIN clips pc ON pc.id = peer.clip_id
    JOIN clips self ON self.id = clip_labels.clip_id
    WHERE peer.label_id = clip_labels.label_id
      AND (pc.uploaded_at > self.uploaded_at
           OR (pc.uploaded_at = self.uploaded_at AND pc.id > self.id))
);

-- Reading a playlist is "this label's rows, in position order", which is
-- this index; the existing idx_clip_labels_label stays for the filter
-- joins that do not care about order.
CREATE INDEX idx_clip_labels_order ON clip_labels(label_id, position);
