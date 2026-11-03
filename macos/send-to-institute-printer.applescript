-- Item for the PDF menu of the macOS print dialog. The print system saves the
-- document as a PDF and opens it with this app, which sends it with
-- institute-print and the printer's default settings.
--
-- Install (see README.md):
--   mkdir -p ~/Library/"PDF Services"
--   osacompile -o ~/Library/"PDF Services/Send to Institute Printer.app" macos/send-to-institute-printer.applescript

-- The print system runs apps with a minimal PATH.
property searchPath : "$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"

on open theFiles
	repeat with theFile in theFiles
		set pdfPath to POSIX path of theFile
		try
			set report to do shell script "PATH=" & searchPath & " institute-print " & quoted form of pdfPath
			display notification report with title "Institute printer"
		on error errorText
			display alert "Could not print" message errorText as critical
		end try
		removeSpoolFile(pdfPath)
	end repeat
end open

-- Deleting the spooled PDF is left to the app. Only files in temporary folders
-- are removed, so a PDF dropped onto the app from elsewhere is kept.
on removeSpoolFile(pdfPath)
	if pdfPath starts with "/private/var/folders/" or pdfPath starts with "/private/tmp/" or pdfPath starts with "/tmp/" then
		do shell script "rm -f " & quoted form of pdfPath
	end if
end removeSpoolFile
