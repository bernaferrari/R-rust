// Ported 1:1 from ./r-source/src/extra/graphapp/status.c - original code structure preserved
// No modifications, exact function names maintained

#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(unused_variables)]
#![allow(unused_mut)]
#![allow(dead_code)]

/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 1999  Guido Masarotto
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 *
 *  This program is distributed in the hope that it will be useful,
 *  but WITHOUT ANY WARRANTY; without even the implied warranty of
 *  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 *  GNU General Public License for more details.
 *
 *  You should have received a copy of the GNU General Public License
 *  along with this program; if not, a copy is available at
 *  https://www.R-project.org/Licenses/
 */

/*
 *  pub unsafe fn addstatusbar()  - add a simple status bar to the mdi frame
 *  pub unsafe fn setstatus(char *text) - set text
 */

/*
   This file is an add-on  to GraphApp, a cross-platform C graphics library.
 */

#include "internal.h"

static char MDIStatusText[256] = "" ;
#ifndef SBARS_SIZEGRIP
#include "commctrl.h"
#endif

static HWND intMDIStatus=0;

pub unsafe fn addstatusbar(void) -> c_int {
{
    int a[1] = {-1};
    if (!MDIFrame) return 0;
    if (MDIStatus) return 1;
    if(!intMDIStatus) {
	InitCommonControls();
	intMDIStatus = CreateStatusWindow(WS_CHILD|SBARS_SIZEGRIP|WS_VISIBLE,
					  "", hwndFrame, 121);
	if (!intMDIStatus) return 0;
	sendmessage(intMDIStatus,SB_SETPARTS,(WPARAM)1,(LPARAM)a);
	sendmessage(intMDIStatus,SB_SETTEXT,
		    (WPARAM) 0|0, (LPARAM)MDIStatusText);
    }
    MDIStatus = intMDIStatus;
    sendmessage(hwndFrame,WM_PAINT,(WPARAM) 0,(LPARAM) 0);
    return 1;
}

pub unsafe fn delstatusbar(void) -> c_int {
{
    if (!MDIFrame) return 0;
    MDIStatus = 0; /* handle_mdiframeresize notices this */
    sendmessage(hwndFrame,WM_PAINT,(WPARAM) 0,(LPARAM) 0);
    return 1;
}

PROTECTED pub unsafe fn updatestatus(const char *text) {
{
    /* strncpy(MDIStatusText, text, 255); */
    if (!MDIStatus) return;
    sendmessage(MDIStatus,SB_SETTEXT,
		(WPARAM) 0|0, (LPARAM)MDIStatusText);
    sendmessage(MDIStatus, WM_PAINT, (WPARAM)0, (LPARAM)0);
}

pub unsafe fn setstatus(const char *text) {
{
    strncpy(MDIStatusText, text, 255);
    if (!MDIStatus || !current_window) return;
    strncpy(current_window->status, text, 255);
    updatestatus(text);
}
