import 'dart:io';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';

import '../api/client.dart';
import 'common.dart';

/// Extensions the server accepts. Offered to the picker so a file that
/// would be refused cannot be chosen in the first place — a rejection
/// after a slow upload is a bad way to learn the rule.
const _audioExtensions = [
  'mp3',
  'm4a',
  'mp4',
  'wav',
  'flac',
  'ogg',
  'oga',
  'opus',
  'aac',
  'webm',
];

/// What the server will take for one clip. Checked here as well so an
/// oversized file is refused before it is sent rather than after.
const _maxBytes = 10 * 1024 * 1024;

/// Pick audio files and upload them. Returns how many clips were
/// created, so the caller knows whether to reload.
///
/// Several files can be chosen at once and go up in one request, which
/// is how the web version works too: a band records a rehearsal and has
/// eight takes, not one.
Future<int> pickAndUpload(BuildContext context) async {
  final FilePickerResult? picked;
  try {
    picked = await FilePicker.platform.pickFiles(
      allowMultiple: true,
      type: FileType.custom,
      allowedExtensions: _audioExtensions,
      withData: false,
    );
  } catch (e) {
    if (context.mounted) {
      showMessage(context, "Couldn't open the file picker.", isError: true);
    }
    return 0;
  }
  if (picked == null || picked.files.isEmpty) return 0;
  if (!context.mounted) return 0;

  final files = <UploadFile>[];
  final tooLarge = <String>[];
  for (final file in picked.files) {
    final path = file.path;
    if (path == null) continue;
    final bytes = await File(path).readAsBytes();
    if (bytes.length > _maxBytes) {
      tooLarge.add(file.name);
      continue;
    }
    files.add(UploadFile(filename: file.name, bytes: bytes));
  }

  if (!context.mounted) return 0;
  if (files.isEmpty) {
    showMessage(
      context,
      tooLarge.isEmpty
          ? "Those files couldn't be read."
          : 'Too large to upload: ${tooLarge.join(', ')}',
      isError: true,
    );
    return 0;
  }

  final api = sessionOf(context).api;
  final uploaded = await showDialog<int>(
    context: context,
    barrierDismissible: false,
    builder: (dialogContext) => _UploadingDialog(
      count: files.length,
      upload: () async => (await api.upload(files)).length,
    ),
  );

  if (!context.mounted) return uploaded ?? 0;
  if (uploaded != null && uploaded > 0) {
    showMessage(
      context,
      tooLarge.isEmpty
          ? 'Uploaded $uploaded ${uploaded == 1 ? 'clip' : 'clips'}.'
          : 'Uploaded $uploaded, skipped ${tooLarge.length} too large.',
    );
  }
  return uploaded ?? 0;
}

/// A modal while the upload is in flight.
///
/// Modal on purpose: the request carries the bytes, and navigating away
/// mid-upload would leave the user with no idea whether it finished.
class _UploadingDialog extends StatefulWidget {
  const _UploadingDialog({required this.count, required this.upload});

  final int count;
  final Future<int> Function() upload;

  @override
  State<_UploadingDialog> createState() => _UploadingDialogState();
}

class _UploadingDialogState extends State<_UploadingDialog> {
  @override
  void initState() {
    super.initState();
    _run();
  }

  Future<void> _run() async {
    try {
      final created = await widget.upload();
      if (mounted) Navigator.of(context).pop(created);
    } on ApiException catch (e) {
      if (!mounted) return;
      Navigator.of(context).pop(0);
      showMessage(context, e.message, isError: true);
    }
  }

  @override
  Widget build(BuildContext context) => AlertDialog(
        content: Row(
          children: [
            const SizedBox(
              height: 20,
              width: 20,
              child: CircularProgressIndicator(strokeWidth: 2),
            ),
            const SizedBox(width: 16),
            Expanded(
              child: Text(widget.count == 1
                  ? 'Uploading 1 clip…'
                  : 'Uploading ${widget.count} clips…'),
            ),
          ],
        ),
      );
}
