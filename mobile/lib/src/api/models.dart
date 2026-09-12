/// The shapes `/api/v1` sends, as Dart.
///
/// Every field is parsed defensively: a server one version ahead can add
/// fields, and a clip whose audio the server could not decode has no
/// peaks and no duration. Nothing here throws on a missing optional.
library;

/// A label, as ids and a name. The server sends no links — what a label
/// means in the UI is the client's business.
class Label {
  const Label({required this.id, required this.name});

  factory Label.fromJson(Map<String, dynamic> json) => Label(
        id: json['id'] as int,
        name: json['name'] as String,
      );

  final int id;
  final String name;

  @override
  bool operator ==(Object other) =>
      other is Label && other.id == id && other.name == name;

  @override
  int get hashCode => Object.hash(id, name);
}

/// A label's clips as a playlist, in the order the band put them in.
class Playlist {
  const Playlist({
    required this.labelId,
    required this.labelName,
    required this.total,
    required this.clips,
  });

  factory Playlist.fromJson(Map<String, dynamic> json) {
    final seconds = json['totalSeconds'];
    return Playlist(
      labelId: json['labelId'] as int,
      labelName: json['labelName'] as String,
      total: Duration(
        microseconds: ((seconds is num ? seconds : 0) * 1000000).round(),
      ),
      clips: ((json['clips'] as List<dynamic>?) ?? const [])
          .map((c) => Clip.fromJson(c as Map<String, dynamic>))
          .toList(growable: false),
    );
  }

  final int labelId;
  final String labelName;

  /// Summed over the clips whose length is known.
  final Duration total;
  final List<Clip> clips;
}

/// One audio clip.
class Clip {
  const Clip({
    required this.id,
    required this.name,
    required this.originalFilename,
    required this.contentType,
    required this.uploadedAt,
    required this.recordingDate,
    required this.uploader,
    required this.labels,
    required this.peaks,
    required this.duration,
    required this.audioPath,
    required this.downloadPath,
    required this.canDelete,
  });

  factory Clip.fromJson(Map<String, dynamic> json) {
    final rawPeaks = json['peaks'];
    final rawDuration = json['durationSeconds'];
    return Clip(
      id: json['id'] as int,
      name: json['name'] as String,
      originalFilename: json['originalFilename'] as String? ?? '',
      contentType: json['contentType'] as String? ?? '',
      uploadedAt: DateTime.parse(json['uploadedAt'] as String).toLocal(),
      recordingDate: json['recordingDate'] as String?,
      uploader: json['uploader'] as String? ?? '',
      labels: ((json['labels'] as List<dynamic>?) ?? const [])
          .map((l) => Label.fromJson(l as Map<String, dynamic>))
          .toList(growable: false),
      peaks: rawPeaks is List
          ? rawPeaks
              .map((p) => (p as num).toDouble())
              .toList(growable: false)
          : null,
      duration: rawDuration is num
          ? Duration(microseconds: (rawDuration * 1000000).round())
          : null,
      audioPath: json['audioUrl'] as String,
      downloadPath: json['downloadUrl'] as String? ?? json['audioUrl'] as String,
      canDelete: json['canDelete'] as bool? ?? false,
    );
  }

  final int id;
  final String name;
  final String originalFilename;
  final String contentType;

  /// When the clip was uploaded, already in the device's zone.
  final DateTime uploadedAt;

  /// The day the recording was made, "YYYY-MM-DD", when the file said
  /// so. Left as a string: it is a civil date with no time and no zone,
  /// and turning it into a DateTime would invent both.
  final String? recordingDate;

  final String uploader;
  final List<Label> labels;

  /// Normalised waveform peaks, or null when the server could not decode
  /// the file. The row then draws a flat line rather than nothing.
  final List<double>? peaks;

  /// Null for the same reason as [peaks]; the player discovers it from
  /// the audio instead.
  final Duration? duration;

  /// Server-relative, e.g. "/clips/3/audio". Resolved against whichever
  /// server this install is pointed at.
  final String audioPath;
  final String downloadPath;

  /// Whether the signed-in user may delete this clip. The server
  /// decides; the app only hides the button.
  final bool canDelete;

  Clip withLabels(List<Label> replacement) => Clip(
        id: id,
        name: name,
        originalFilename: originalFilename,
        contentType: contentType,
        uploadedAt: uploadedAt,
        recordingDate: recordingDate,
        uploader: uploader,
        labels: replacement,
        peaks: peaks,
        duration: duration,
        audioPath: audioPath,
        downloadPath: downloadPath,
        canDelete: canDelete,
      );

  Clip withName(String replacement) => Clip(
        id: id,
        name: replacement,
        originalFilename: originalFilename,
        contentType: contentType,
        uploadedAt: uploadedAt,
        recordingDate: recordingDate,
        uploader: uploader,
        labels: labels,
        peaks: peaks,
        duration: duration,
        audioPath: audioPath,
        downloadPath: downloadPath,
        canDelete: canDelete,
      );
}

/// The signed-in user.
class User {
  const User({required this.id, required this.username, required this.isAdmin});

  factory User.fromJson(Map<String, dynamic> json) => User(
        id: json['id'] as int,
        username: json['username'] as String,
        isAdmin: json['isAdmin'] as bool? ?? false,
      );

  final int id;
  final String username;
  final bool isAdmin;
}

/// A label's wiki page. [content] is Markdown source, not HTML.
class WikiPage {
  const WikiPage({
    required this.labelId,
    required this.labelName,
    required this.content,
    required this.hasContent,
    required this.lastEditedBy,
    required this.lastEditedAt,
  });

  factory WikiPage.fromJson(Map<String, dynamic> json) {
    final at = json['lastEditedAt'] as String?;
    return WikiPage(
      labelId: json['labelId'] as int,
      labelName: json['labelName'] as String,
      content: json['content'] as String? ?? '',
      hasContent: json['hasContent'] as bool? ?? false,
      lastEditedBy: json['lastEditedBy'] as String?,
      lastEditedAt: at == null ? null : DateTime.parse(at).toLocal(),
    );
  }

  final int labelId;
  final String labelName;
  final String content;
  final bool hasContent;
  final String? lastEditedBy;
  final DateTime? lastEditedAt;
}

/// One entry in a wiki page's history.
class WikiRevision {
  const WikiRevision({
    required this.id,
    required this.author,
    required this.editedAt,
    required this.content,
    required this.isCurrent,
  });

  factory WikiRevision.fromJson(Map<String, dynamic> json) => WikiRevision(
        id: json['id'] as int,
        author: json['author'] as String,
        editedAt: DateTime.parse(json['editedAt'] as String).toLocal(),
        content: json['content'] as String? ?? '',
        isCurrent: json['isCurrent'] as bool? ?? false,
      );

  final int id;
  final String author;
  final DateTime editedAt;
  final String content;
  final bool isCurrent;
}

/// What the label picker offers for what the user has typed.
class LabelSuggestions {
  const LabelSuggestions({
    required this.query,
    required this.matches,
    required this.canCreate,
  });

  factory LabelSuggestions.fromJson(Map<String, dynamic> json) =>
      LabelSuggestions(
        query: json['query'] as String? ?? '',
        matches: ((json['matches'] as List<dynamic>?) ?? const [])
            .map((m) => m as String)
            .toList(growable: false),
        canCreate: json['canCreate'] as bool? ?? false,
      );

  /// The normalised query the suggestions were computed for — the name
  /// to create when [canCreate] is set.
  final String query;
  final List<String> matches;
  final bool canCreate;

  static const empty =
      LabelSuggestions(query: '', matches: [], canCreate: false);
}

/// One of the user's signed-in devices.
class Device {
  const Device({
    required this.id,
    required this.name,
    required this.createdAt,
    required this.lastUsedOn,
    required this.isCurrent,
  });

  factory Device.fromJson(Map<String, dynamic> json) => Device(
        id: json['id'] as int,
        name: json['deviceName'] as String? ?? 'Unnamed device',
        createdAt: DateTime.parse(json['createdAt'] as String).toLocal(),
        lastUsedOn: json['lastUsedOn'] as String?,
        isCurrent: json['isCurrent'] as bool? ?? false,
      );

  final int id;
  final String name;
  final DateTime createdAt;

  /// "YYYY-MM-DD", or null if the device has not been used since it was
  /// signed in. A date rather than an instant: the server only records
  /// the day, so it does not write on every request.
  final String? lastUsedOn;

  /// Whether this is the device asking.
  final bool isCurrent;
}

/// A clip created by an upload.
class UploadedClip {
  const UploadedClip({required this.id, required this.name});

  factory UploadedClip.fromJson(Map<String, dynamic> json) => UploadedClip(
        id: json['id'] as int,
        name: json['name'] as String,
      );

  final int id;
  final String name;
}

/// A token and the user it belongs to, as returned by signing in.
class SignIn {
  const SignIn({required this.token, required this.user});

  final String token;
  final User user;
}
