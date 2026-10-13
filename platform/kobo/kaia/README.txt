Kaia Reader for Kobo
====================

Kaia Reader is a KOReader fork. This package installs it next to stock
KOReader. It uses its own folder, its own settings and its own launch
entry. It does not update or replace KOReader.

What the package contains

  .adds/kaia/        Kaia Reader, the launcher kaia.sh, and the settings folder
  .adds/nm/kaia      NickelMenu entry "Kaia Reader"

Install

1. Connect the Kobo to the computer with USB. Wait until the storage mounts.
2. Unzip this package at the root of the Kobo storage. Keep the folder
   structure. The package adds .adds/kaia and .adds/nm/kaia. It does not
   change .adds/koreader.
3. Eject the Kobo in the file manager, then unplug the cable.
4. Open NickelMenu on the Kobo and tap "Kaia Reader".

NickelMenu is not part of this package. Install it separately and follow its
own documentation. NickelMenu does not support firmware 5.x. On firmware 5.x,
use another launcher, such as KFMon, and see its documentation.

Safe mode

Kaia starts in safe mode when the file .adds/kaia/safemode exists. In safe
mode, third-party plugins and user patches are off. To start safe mode, create
an empty file named safemode in .adds/kaia over USB. To leave safe mode,
delete that file and restart Kaia.

Crash log

Kaia writes its log to .adds/kaia/crash.log.

Uninstall

Delete the folder .adds/kaia and the file .adds/nm/kaia. Deleting .adds/kaia
also deletes Kaia's settings and sticker data. Copy the folder elsewhere first
if you want to keep them.

Status

Not yet tested on a device. The package has no Kaia artwork yet.
